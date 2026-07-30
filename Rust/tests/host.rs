//! Tests for the hardware-independent part of the firmware.
//!
//! This is an integration-test crate: it consumes the public library exactly
//! like the embedded binary does. Cargo only enables it with the `host-tests`
//! feature, keeping Rust's standard test harness away from the bare-metal
//! target selected by VS Code.

use nucleo_h723zg_udp_echo::{
    FALLBACK_ADDRESS, FALLBACK_GATEWAY, FALLBACK_PREFIX_LENGTH, MAC_ADDRESS, MAX_DATAGRAM_SIZE,
    SSH_PORT, SSH_USERNAME, SshCommand, echo_payload, parse_ssh_command,
};

#[test]
fn empty_datagram_produces_an_empty_reply() {
    assert_eq!(echo_payload(&[0xaa; 8], 0), Some(&[][..]));
}

#[test]
fn reply_contains_only_received_bytes() {
    let buffer = [1, 2, 3, 4, 0, 0];
    assert_eq!(echo_payload(&buffer, 4), Some(&[1, 2, 3, 4][..]));
}

#[test]
fn full_receive_buffer_is_valid() {
    let buffer = [0x5a; MAX_DATAGRAM_SIZE];
    assert_eq!(echo_payload(&buffer, MAX_DATAGRAM_SIZE), Some(&buffer[..]));
}

#[test]
fn length_beyond_buffer_is_rejected() {
    assert_eq!(echo_payload(&[1, 2, 3], 4), None);
}

#[test]
fn mac_address_is_local_and_unicast() {
    assert_eq!(MAC_ADDRESS[0] & 0b0000_0010, 0b0000_0010);
    assert_eq!(MAC_ADDRESS[0] & 0b0000_0001, 0);
}

#[test]
fn fallback_gateway_is_in_the_same_24_bit_subnet() {
    assert_eq!(FALLBACK_PREFIX_LENGTH, 24);
    assert_eq!(FALLBACK_ADDRESS[..3], FALLBACK_GATEWAY[..3]);
    assert_ne!(FALLBACK_ADDRESS, FALLBACK_GATEWAY);
}

#[test]
fn buffer_accepts_a_standard_unfragmented_udp_payload() {
    const ETHERNET_MTU: usize = 1500;
    const IPV4_HEADER: usize = 20;
    const UDP_HEADER: usize = 8;

    assert!(MAX_DATAGRAM_SIZE >= ETHERNET_MTU - IPV4_HEADER - UDP_HEADER);
}

#[test]
fn ssh_endpoint_uses_the_documented_unprivileged_port() {
    assert_eq!(SSH_PORT, 2222);
    assert_eq!(SSH_USERNAME, "board");
}

#[test]
fn ssh_parser_recognizes_management_commands() {
    assert_eq!(parse_ssh_command("help"), SshCommand::Help);
    assert_eq!(parse_ssh_command(" status\r\n"), SshCommand::Status);
    assert_eq!(parse_ssh_command("logout"), SshCommand::Exit);
}

#[test]
fn ssh_echo_preserves_the_message() {
    assert_eq!(
        parse_ssh_command("echo hello from Rust"),
        SshCommand::Echo("hello from Rust")
    );
    assert_eq!(parse_ssh_command("echo"), SshCommand::Echo(""));
}

#[test]
fn ssh_parser_reports_unknown_commands() {
    assert_eq!(parse_ssh_command("reboot"), SshCommand::Unknown("reboot"));
}
