//! Hardware-independent rules shared by the firmware and host unit tests.
//!
//! Keeping this module free of STM32 and Embassy types lets `cargo test` build
//! it for Windows or Linux with Rust's normal test harness. The embedded binary
//! consumes the same constants and payload-boundary function, so the tests
//! exercise production code rather than a separate simulation.

#![no_std]

/// Locally administered, unicast Ethernet address used by this example.
pub const MAC_ADDRESS: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

/// UDP port assigned to the traditional echo protocol.
pub const UDP_ECHO_PORT: u16 = 7;

/// Unprivileged TCP port used by the embedded SSH server.
pub const SSH_PORT: u16 = 2222;

/// The single account accepted by the SSH service.
pub const SSH_USERNAME: &str = "board";

/// Storage reserved for one received UDP datagram.
pub const MAX_DATAGRAM_SIZE: usize = 1536;

/// IPv4 configuration used when DHCP does not respond.
pub const FALLBACK_ADDRESS: [u8; 4] = [192, 168, 0, 10];
pub const FALLBACK_GATEWAY: [u8; 4] = [192, 168, 0, 1];
pub const FALLBACK_PREFIX_LENGTH: u8 = 24;

/// Return exactly the initialized portion of a receive buffer.
///
/// Embassy guarantees that a successful receive length fits its destination
/// buffer. Returning `Option` keeps this boundary checked if the function is
/// reused with another packet source in the future.
pub fn echo_payload(buffer: &[u8], received_length: usize) -> Option<&[u8]> {
    buffer.get(..received_length)
}

/// A command understood by the small SSH management shell.
///
/// The variants borrow text from the input line. No heap allocation or copy is
/// necessary, and the caller cannot accidentally retain a command after its
/// input buffer is reused.
#[derive(Debug, PartialEq, Eq)]
pub enum SshCommand<'a> {
    Help,
    Status,
    Echo(&'a str),
    Exit,
    Unknown(&'a str),
}

/// Parse one command line received through SSH.
///
/// This pure function is shared by the firmware and host tests. Keeping
/// protocol-independent decisions here makes them easy to exercise without an
/// STM32, Ethernet cable, or SSH client.
pub fn parse_ssh_command(line: &str) -> SshCommand<'_> {
    let line = line.trim();
    match line {
        "help" => SshCommand::Help,
        "status" => SshCommand::Status,
        "exit" | "logout" => SshCommand::Exit,
        "echo" => SshCommand::Echo(""),
        _ => match line.strip_prefix("echo ") {
            Some(text) => SshCommand::Echo(text),
            None => SshCommand::Unknown(line),
        },
    }
}
