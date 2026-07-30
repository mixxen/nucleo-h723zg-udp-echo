//! Small, public-key-authenticated SSH management service.
//!
//! Sunset implements the SSH protocol and cryptography. Embassy supplies the
//! TCP socket and executor. This module joins those layers, authenticates one
//! provisioned Ed25519 key, and maps a shell or `ssh host command` request onto
//! the hardware-independent command parser in `lib.rs`.

use core::fmt::Write as _;

use defmt::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use heapless::{String, Vec};
use nucleo_h723zg_udp_echo::{SSH_PORT, SSH_USERNAME, SshCommand, parse_ssh_command};
use sunset::{ChanFail, ChanHandle, PubKey, ServEvent, SignKey};
use sunset_async::{ProgressHolder, SSHServer};

// `build.rs` creates this file from Git-ignored, locally provisioned keys.
include!(concat!(env!("OUT_DIR"), "/ssh_keys.rs"));

const TCP_BUFFER_SIZE: usize = 1550;
const SSH_PACKET_BUFFER_SIZE: usize = 4096;
const COMMAND_CAPACITY: usize = 128;

/// A successfully authenticated request waiting for its SSH byte stream.
enum SessionRequest {
    Shell(ChanHandle),
    Exec(ChanHandle, String<COMMAND_CAPACITY>),
}

/// Listen forever, serving one SSH connection at a time.
#[embassy_executor::task]
pub async fn run(stack: Stack<'static>) -> ! {
    // The same host key is reconstructed after every reset. OpenSSH can
    // therefore detect an impostor instead of seeing a new server each boot.
    let host_key = SignKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&SSH_HOST_KEY_SEED));
    let mut tcp_rx = [0; TCP_BUFFER_SIZE];
    let mut tcp_tx = [0; TCP_BUFFER_SIZE];

    loop {
        // Avoid listening before DHCP (or the static fallback) makes the
        // interface reachable.
        stack.wait_config_up().await;
        // An Embassy TCP socket may not be returned to LISTEN after every SSH
        // shutdown path. Recreating this lightweight handle resets its state;
        // the larger packet buffers above are still reused.
        let mut socket = TcpSocket::new(stack, &mut tcp_rx, &mut tcp_tx);
        socket.set_nagle_enabled(false);
        info!("SSH listening on TCP port {}", SSH_PORT);

        if socket.accept(SSH_PORT).await.is_err() {
            warn!("SSH TCP accept failed");
            // Do not let an unexpected immediate error monopolize the
            // cooperative executor and starve the network driver.
            Timer::after(Duration::from_millis(100)).await;
            continue;
        }

        if let Some(remote) = socket.remote_endpoint() {
            info!("SSH connection from {}", remote);
        }
        match serve_connection(&mut socket, stack, &host_key).await {
            Ok(()) => socket.close(),
            Err(_) => {
                warn!("SSH connection ended with an error");
                socket.abort();
            }
        }

        // `close` queues a FIN and `abort` queues a reset. In either case,
        // waiting here prevents the client from being left half-open.
        let _ = socket.flush().await;
    }
}

/// Connect one accepted TCP stream to one Sunset server instance.
async fn serve_connection(
    socket: &mut TcpSocket<'_>,
    stack: Stack<'static>,
    host_key: &SignKey,
) -> sunset::Result<()> {
    // Sunset uses caller-owned packet buffers, keeping memory use visible and
    // deterministic. These are distinct from Embassy's TCP window buffers.
    let mut ssh_rx = [0; SSH_PACKET_BUFFER_SIZE];
    let mut ssh_tx = [0; SSH_PACKET_BUFFER_SIZE];
    let server = SSHServer::new(&mut ssh_rx, &mut ssh_tx);
    let requests = Channel::<NoopRawMutex, SessionRequest, 1>::new();

    let protocol = handle_protocol_events(&server, host_key, &requests);
    let commands = handle_session(&server, stack, &requests);
    let application = async {
        match select(protocol, commands).await {
            Either::First(result) | Either::Second(result) => result,
        }
    };

    let (mut reader, mut writer) = socket.split();
    let transport = server.run(&mut reader, &mut writer);

    // The transport pumps encrypted bytes; the application responds to parsed
    // SSH events. Either side finishing tears down the whole connection.
    match select(transport, application).await {
        Either::First(result) | Either::Second(result) => result,
    }
}

/// Authenticate the peer and turn accepted channel requests into work.
async fn handle_protocol_events(
    server: &SSHServer<'_>,
    host_key: &SignKey,
    requests: &Channel<NoopRawMutex, SessionRequest, 1>,
) -> sunset::Result<()> {
    let mut opened_channel: Option<ChanHandle> = None;

    loop {
        // ProgressHolder temporarily owns Sunset's internal session lock while
        // an event is inspected and answered.
        let mut progress = ProgressHolder::new();
        match server.progress(&mut progress).await? {
            ServEvent::Hostkeys(event) => event.hostkeys(&[host_key])?,
            ServEvent::FirstAuth(mut event) => {
                // Advertise only public-key authentication. Dropping or
                // rejecting this first probe asks the client for its key.
                event.set_auth_methods(false, true)?;
                event.reject()?;
            }
            ServEvent::PasswordAuth(event) => event.reject()?,
            ServEvent::PubkeyAuth(event) => {
                let correct_user = event.username()? == SSH_USERNAME;
                let correct_key = match event.pubkey()? {
                    PubKey::Ed25519(key) => key.key.0 == SSH_AUTHORIZED_KEY,
                    _ => false,
                };
                if correct_user && correct_key {
                    event.allow()?;
                } else {
                    event.reject()?;
                }
            }
            ServEvent::Authenticated => info!("SSH public-key authentication succeeded"),
            ServEvent::OpenSession(event) => {
                if opened_channel.is_none() {
                    opened_channel = Some(event.accept()?);
                } else {
                    event.reject(ChanFail::SSH_OPEN_ADMINISTRATIVELY_PROHIBITED)?;
                }
            }
            ServEvent::SessionPty(event) => event.succeed()?,
            ServEvent::SessionEnv(event) => event.succeed()?,
            ServEvent::SessionShell(event) => {
                let Some(handle) = opened_channel.take() else {
                    event.fail()?;
                    continue;
                };
                if handle.num() != event.channel()
                    || requests.try_send(SessionRequest::Shell(handle)).is_err()
                {
                    event.fail()?;
                } else {
                    event.succeed()?;
                }
            }
            ServEvent::SessionExec(event) => {
                let command = String::try_from(event.command()?)
                    .map_err(|_| sunset::Error::msg("SSH command is too long"))?;
                let Some(handle) = opened_channel.take() else {
                    event.fail()?;
                    continue;
                };
                if handle.num() != event.channel()
                    || requests
                        .try_send(SessionRequest::Exec(handle, command))
                        .is_err()
                {
                    event.fail()?;
                } else {
                    event.succeed()?;
                }
            }
            ServEvent::SessionSubsystem(event) => event.fail()?,
            ServEvent::Defunct => return Ok(()),
            ServEvent::PollAgain => {}
        }
    }
}

/// Attach Sunset's accepted channel to an interactive or one-shot command.
async fn handle_session(
    server: &SSHServer<'_>,
    stack: Stack<'static>,
    requests: &Channel<NoopRawMutex, SessionRequest, 1>,
) -> sunset::Result<()> {
    match requests.receive().await {
        SessionRequest::Shell(handle) => {
            let mut stream = server.stdio(handle).await?;
            interactive_shell(&mut stream, stack).await?;
        }
        SessionRequest::Exec(handle, command) => {
            let mut stream = server.stdio(handle).await?;
            execute_command(&mut stream, stack, &command).await?;
        }
    }

    // Dropping the channel stream marks it done. Sunset 0.5 does not expose a
    // server-side exit-status/close method, so keep its protocol and transport
    // loops alive briefly to drain the encrypted reply and any peer EOF before
    // the caller finishes TCP with a graceful FIN.
    Timer::after(Duration::from_millis(250)).await;
    Ok(())
}

/// Read terminal input a byte at a time and execute complete lines.
async fn interactive_shell(
    stream: &mut (impl Read<Error = sunset::Error> + Write<Error = sunset::Error>),
    stack: Stack<'static>,
) -> sunset::Result<()> {
    stream
        .write_all(
            b"\r\nNUCLEO-H723ZG Rust management shell\r\n\
              Type 'help' for commands.\r\n> ",
        )
        .await?;

    let mut line = Vec::<u8, COMMAND_CAPACITY>::new();
    let mut input = [0; 32];
    let mut previous_was_cr = false;

    loop {
        let count = stream.read(&mut input).await?;
        if count == 0 {
            return Ok(());
        }

        for &byte in &input[..count] {
            match byte {
                // OpenSSH commonly sends CRLF. Treat that pair as one line.
                b'\n' if previous_was_cr => {
                    previous_was_cr = false;
                }
                b'\r' | b'\n' => {
                    previous_was_cr = byte == b'\r';
                    stream.write_all(b"\r\n").await?;
                    let text = core::str::from_utf8(&line)
                        .map_err(|_| sunset::Error::msg("command is not UTF-8"))?;
                    if !execute_command(stream, stack, text).await? {
                        return Ok(());
                    }
                    line.clear();
                    stream.write_all(b"> ").await?;
                }
                0x04 => return Ok(()), // Ctrl-D
                0x08 | 0x7f => {
                    previous_was_cr = false;
                    if line.pop().is_some() {
                        stream.write_all(b"\x08 \x08").await?;
                    }
                }
                0x20..=0x7e => {
                    previous_was_cr = false;
                    if line.push(byte).is_ok() {
                        stream.write_all(&[byte]).await?;
                    }
                }
                _ => previous_was_cr = false,
            }
        }
    }
}

/// Execute one parsed management command, returning false to close the shell.
async fn execute_command(
    output: &mut impl Write<Error = sunset::Error>,
    stack: Stack<'static>,
    line: &str,
) -> sunset::Result<bool> {
    match parse_ssh_command(line) {
        SshCommand::Help => {
            output
                .write_all(
                    b"help          show this command list\r\n\
                      status        show Ethernet and IPv4 state\r\n\
                      echo TEXT     write TEXT back\r\n\
                      exit          close the SSH session\r\n",
                )
                .await?;
        }
        SshCommand::Status => write_status(output, stack).await?,
        SshCommand::Echo(text) => {
            output.write_all(text.as_bytes()).await?;
            output.write_all(b"\r\n").await?;
        }
        SshCommand::Exit => {
            output.write_all(b"bye\r\n").await?;
            return Ok(false);
        }
        SshCommand::Unknown(command) => {
            output.write_all(b"unknown command: ").await?;
            output.write_all(command.as_bytes()).await?;
            output.write_all(b"\r\n").await?;
        }
    }
    Ok(true)
}

/// Format live network state without heap allocation.
async fn write_status(
    output: &mut impl Write<Error = sunset::Error>,
    stack: Stack<'static>,
) -> sunset::Result<()> {
    let mut text = String::<192>::new();
    let _ = writeln!(
        text,
        "Ethernet link: {}\r",
        if stack.is_link_up() { "up" } else { "down" }
    );
    if let Some(config) = stack.config_v4() {
        let _ = writeln!(
            text,
            "IPv4 address: {}/{}\r",
            config.address.address(),
            config.address.prefix_len()
        );
        if let Some(gateway) = config.gateway {
            let _ = writeln!(text, "Gateway: {gateway}\r");
        }
    } else {
        let _ = writeln!(text, "IPv4 address: not configured\r");
    }
    let _ = writeln!(text, "UDP echo: port 7\r");
    let _ = writeln!(text, "SSH: port {SSH_PORT}\r");
    output.write_all(text.as_bytes()).await
}
