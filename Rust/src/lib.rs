//! Hardware-independent rules shared by the firmware and host unit tests.
//!
//! Keeping this module free of STM32 and Embassy types lets `cargo test` build
//! it for Windows or Linux with Rust's normal test harness. The embedded binary
//! consumes the same constants and payload-boundary function, so the tests
//! exercise production code rather than a separate simulation.

#![no_std]

/// Locally administered, unicast Ethernet address used by this example.
pub const MAC_ADDRESS: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Locally administered address used only by the SPI-connected W5500 shield.
///
/// Keeping this distinct from [`MAC_ADDRESS`] gives the router separate DHCP
/// identities for the native RMII port and the removable shield.
pub const W5500_MAC_ADDRESS: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x55, 0x00];

/// UDP port assigned to the traditional echo protocol.
pub const UDP_ECHO_PORT: u16 = 7;

/// UDP endpoint used only by profiling benchmark images.
pub const PROFILING_PORT: u16 = 5001;
pub const PROFILING_MAGIC: [u8; 8] = *b"STRMPRF1";
pub const PROFILING_WIRE_SIZE: usize = 48;

/// Unprivileged TCP port used by the embedded SSH server.
pub const SSH_PORT: u16 = 2222;

/// The single account accepted by the SSH service.
pub const SSH_USERNAME: &str = "board";

/// Name passed to `ssh -s` for the isolated firmware-upload protocol.
pub const FIRMWARE_UPDATE_SUBSYSTEM: &str = "firmware-update";

/// Wire-format marker and version for a firmware upload header.
pub const UPDATE_PROTOCOL_MAGIC: [u8; 4] = *b"FWUP";
pub const UPDATE_PROTOCOL_VERSION: u8 = 1;
pub const UPDATE_HEADER_SIZE: usize = 44;

/// Largest signed image that can later fit in MCUboot's active image area.
pub const MAX_SIGNED_IMAGE_SIZE: u32 = 0x0004_0000;

/// MCUboot's image-header magic, represented as it appears in the binary.
pub const MCUBOOT_IMAGE_MAGIC: [u8; 4] = [0x3d, 0xb8, 0xf3, 0x96];

/// Internal-flash offsets used by the pinned MCUboot offset-swap layout.
///
/// These are offsets from `0x0800_0000`, which is also what Embassy's flash
/// driver expects. The first secondary sector is MCUboot's swap workspace, so
/// uploaded image bytes begin one 128 KiB sector into that partition.
pub const PRIMARY_SLOT_OFFSET: u32 = 0x0002_0000;
pub const PRIMARY_SLOT_SIZE: u32 = 0x0006_0000;
pub const SECONDARY_SLOT_OFFSET: u32 = 0x0008_0000;
pub const SECONDARY_SLOT_SIZE: u32 = 0x0008_0000;
pub const STAGED_IMAGE_OFFSET: u32 = 0x000a_0000;
pub const FLASH_WRITE_SIZE: usize = 32;

/// Exact 16-byte magic selected by MCUboot when its maximum alignment is 32.
pub const MCUBOOT_TRAILER_MAGIC: [u8; 16] = [
    0x20, 0x00, 0x2d, 0xe1, 0x5d, 0x29, 0x41, 0x0b, 0x8d, 0x77, 0x67, 0x9c, 0x11, 0x0f, 0x1f, 0x8a,
];

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

/// Metadata sent before the signed image bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct UpdateHeader {
    pub image_length: u32,
    pub sha256: [u8; 32],
}

/// Why an upload header was rejected before flash was touched.
#[derive(Debug, PartialEq, Eq)]
pub enum UpdateHeaderError {
    WrongMagic,
    UnsupportedVersion,
    NonzeroReservedBytes,
    InvalidLength,
}

impl UpdateHeader {
    /// Decode the fixed 44-byte, little-endian upload header.
    pub fn decode(bytes: &[u8; UPDATE_HEADER_SIZE]) -> Result<Self, UpdateHeaderError> {
        if bytes[..4] != UPDATE_PROTOCOL_MAGIC {
            return Err(UpdateHeaderError::WrongMagic);
        }
        if bytes[4] != UPDATE_PROTOCOL_VERSION {
            return Err(UpdateHeaderError::UnsupportedVersion);
        }
        if bytes[5..8] != [0; 3] {
            return Err(UpdateHeaderError::NonzeroReservedBytes);
        }

        let image_length = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        if !(0x200..=MAX_SIGNED_IMAGE_SIZE).contains(&image_length) {
            return Err(UpdateHeaderError::InvalidLength);
        }

        Ok(Self {
            image_length,
            sha256: bytes[12..].try_into().unwrap(),
        })
    }
}

/// Check the first bytes before treating an upload as an MCUboot image.
pub fn has_mcuboot_image_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&MCUBOOT_IMAGE_MAGIC)
}

/// Start of the aligned 32-byte trailer write containing MCUboot's magic.
pub const fn trailer_magic_block_offset(slot_offset: u32, slot_size: u32) -> u32 {
    slot_offset + slot_size - 32
}

/// Start of the trailer's `image_ok` write block.
pub const fn trailer_image_ok_offset(slot_offset: u32, slot_size: u32) -> u32 {
    trailer_magic_block_offset(slot_offset, slot_size) - 32
}

/// Start of the trailer's `swap_info` write block.
pub const fn trailer_swap_info_offset(slot_offset: u32, slot_size: u32) -> u32 {
    trailer_image_ok_offset(slot_offset, slot_size) - 64
}

/// Build an aligned flash word that marks a secondary image as present.
pub fn mcuboot_magic_block() -> [u8; FLASH_WRITE_SIZE] {
    let mut block = [0xff; FLASH_WRITE_SIZE];
    block[16..].copy_from_slice(&MCUBOOT_TRAILER_MAGIC);
    block
}

/// Build MCUboot's test-upgrade swap-info word for image number zero.
pub fn mcuboot_test_swap_info_block() -> [u8; FLASH_WRITE_SIZE] {
    let mut block = [0xff; FLASH_WRITE_SIZE];
    block[0] = 2; // BOOT_SWAP_TYPE_TEST, image number zero.
    block
}

/// Build the one-way flash word used to confirm a healthy trial image.
pub fn mcuboot_image_ok_block() -> [u8; FLASH_WRITE_SIZE] {
    let mut block = [0xff; FLASH_WRITE_SIZE];
    block[0] = 1; // BOOT_FLAG_SET
    block
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
