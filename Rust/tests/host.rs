//! Tests for the hardware-independent part of the firmware.
//!
//! This is an integration-test crate: it consumes the public library exactly
//! like the embedded binary does. Cargo only enables it with the `host-tests`
//! feature, keeping Rust's standard test harness away from the bare-metal
//! target selected by VS Code.

use nucleo_h723zg_udp_echo::{
    FALLBACK_ADDRESS, FALLBACK_GATEWAY, FALLBACK_PREFIX_LENGTH, FLASH_WRITE_SIZE, MAC_ADDRESS,
    MAX_DATAGRAM_SIZE, MAX_SIGNED_IMAGE_SIZE, MCUBOOT_IMAGE_MAGIC, MCUBOOT_TRAILER_MAGIC,
    PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE, SECONDARY_SLOT_OFFSET, SECONDARY_SLOT_SIZE, SSH_PORT,
    SSH_USERNAME, SshCommand, UPDATE_HEADER_SIZE, UPDATE_PROTOCOL_MAGIC, UPDATE_PROTOCOL_VERSION,
    UpdateHeader, UpdateHeaderError, echo_payload, has_mcuboot_image_magic, mcuboot_image_ok_block,
    mcuboot_magic_block, mcuboot_test_swap_info_block, parse_ssh_command, trailer_image_ok_offset,
    trailer_magic_block_offset, trailer_swap_info_offset,
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

fn encoded_update_header(length: u32) -> [u8; UPDATE_HEADER_SIZE] {
    let mut bytes = [0; UPDATE_HEADER_SIZE];
    bytes[..4].copy_from_slice(&UPDATE_PROTOCOL_MAGIC);
    bytes[4] = UPDATE_PROTOCOL_VERSION;
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes[12..].copy_from_slice(&[0x5a; 32]);
    bytes
}

#[test]
fn update_header_round_trips_length_and_digest() {
    let decoded = UpdateHeader::decode(&encoded_update_header(123_456)).unwrap();
    assert_eq!(decoded.image_length, 123_456);
    assert_eq!(decoded.sha256, [0x5a; 32]);
}

#[test]
fn update_header_rejects_wrong_magic_version_reserved_and_size() {
    let mut bytes = encoded_update_header(4096);
    bytes[0] ^= 1;
    assert_eq!(
        UpdateHeader::decode(&bytes),
        Err(UpdateHeaderError::WrongMagic)
    );

    let mut bytes = encoded_update_header(4096);
    bytes[4] += 1;
    assert_eq!(
        UpdateHeader::decode(&bytes),
        Err(UpdateHeaderError::UnsupportedVersion)
    );

    let mut bytes = encoded_update_header(4096);
    bytes[7] = 1;
    assert_eq!(
        UpdateHeader::decode(&bytes),
        Err(UpdateHeaderError::NonzeroReservedBytes)
    );

    assert_eq!(
        UpdateHeader::decode(&encoded_update_header(MAX_SIGNED_IMAGE_SIZE + 1)),
        Err(UpdateHeaderError::InvalidLength)
    );
}

#[test]
fn mcuboot_image_marker_is_little_endian() {
    let mut image = [0xff; 32];
    image[..4].copy_from_slice(&MCUBOOT_IMAGE_MAGIC);
    assert!(has_mcuboot_image_magic(&image));
    image[0] ^= 1;
    assert!(!has_mcuboot_image_magic(&image));
}

#[test]
fn trailer_offsets_match_the_pinned_32_byte_alignment() {
    assert_eq!(
        trailer_magic_block_offset(SECONDARY_SLOT_OFFSET, SECONDARY_SLOT_SIZE),
        0x000f_ffe0
    );
    assert_eq!(
        trailer_image_ok_offset(PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE),
        0x0007_ffc0
    );
    assert_eq!(
        trailer_swap_info_offset(SECONDARY_SLOT_OFFSET, SECONDARY_SLOT_SIZE),
        0x000f_ff80
    );
}

#[test]
fn trailer_words_match_mcuboot_fixtures() {
    let magic = mcuboot_magic_block();
    assert_eq!(magic.len(), FLASH_WRITE_SIZE);
    assert_eq!(magic[..16], [0xff; 16]);
    assert_eq!(magic[16..], MCUBOOT_TRAILER_MAGIC);

    let swap_info = mcuboot_test_swap_info_block();
    assert_eq!(swap_info[0], 2);
    assert!(swap_info[1..].iter().all(|byte| *byte == 0xff));

    let image_ok = mcuboot_image_ok_block();
    assert_eq!(image_ok[0], 1);
    assert!(image_ok[1..].iter().all(|byte| *byte == 0xff));
}
