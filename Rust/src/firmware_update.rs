//! Authenticated, bounded firmware staging for MCUboot.
//!
//! SSH authenticates the operator before this module sees any bytes. This
//! module then accepts one signed MCUboot image, writes it only to the
//! secondary slot, verifies it twice (stream and flash read-back), and writes
//! MCUboot's test-upgrade marker last. MCUboot verifies the Ed25519 signature
//! after reset and rolls back unless the new application confirms itself.

use core::cmp::min;
use core::fmt::Write as _;

use embassy_stm32::flash::{Blocking, Flash};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use heapless::String;
use nucleo_h723zg_udp_echo::{
    FLASH_WRITE_SIZE, MCUBOOT_TRAILER_MAGIC, PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE,
    SECONDARY_SLOT_OFFSET, SECONDARY_SLOT_SIZE, STAGED_IMAGE_OFFSET, UPDATE_HEADER_SIZE,
    UpdateHeader, has_mcuboot_image_magic, mcuboot_image_ok_block, mcuboot_magic_block,
    mcuboot_test_swap_info_block, trailer_image_ok_offset, trailer_magic_block_offset,
    trailer_swap_info_offset,
};
use sha2::{Digest, Sha256};

const TRANSFER_BUFFER_SIZE: usize = 1024;
const PROGRESS_INTERVAL: u32 = 16 * 1024;

/// Confirm the running image only when MCUboot says this is a trial boot.
///
/// An ordinary factory-installed image has an erased trailer, so this is a
/// no-op. After a successful test swap, MCUboot leaves its magic in the active
/// trailer and waits for the application to program `image_ok = 1`.
#[cfg_attr(feature = "rollback-test", allow(dead_code))]
pub fn confirm_running_trial(flash: &mut Flash<'static, Blocking>) {
    let magic_block = trailer_magic_block_offset(PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE);
    let mut magic = [0; 16];
    if flash.blocking_read(magic_block + 16, &mut magic).is_err() || magic != MCUBOOT_TRAILER_MAGIC
    {
        return;
    }

    let image_ok = trailer_image_ok_offset(PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE);
    let mut current = [0; 1];
    if flash.blocking_read(image_ok, &mut current).is_ok() && current[0] != 1 {
        // Flash bits only move from one to zero. The rest of this aligned word
        // remains erased, matching MCUboot's own `boot_write_image_ok`.
        let _ = flash.blocking_write(image_ok, &mcuboot_image_ok_block());
    }
}

/// Receive, verify, stage, and activate one signed image.
pub async fn receive(
    stream: &mut (impl Read<Error = sunset::Error> + Write<Error = sunset::Error>),
    flash: &mut Flash<'static, Blocking>,
) -> sunset::Result<()> {
    let mut encoded_header = [0; UPDATE_HEADER_SIZE];
    read_exact(stream, &mut encoded_header).await?;
    let header = match UpdateHeader::decode(&encoded_header) {
        Ok(header) => header,
        Err(_) => {
            stream.write_all(b"ERR invalid header\n").await?;
            return Ok(());
        }
    };

    // Erasing only the secondary partition is the central safety boundary:
    // neither MCUboot nor the currently running primary image can be touched.
    stream.write_all(b"ERASING\n").await?;
    stream.flush().await?;
    if flash
        .blocking_erase(
            SECONDARY_SLOT_OFFSET,
            SECONDARY_SLOT_OFFSET + SECONDARY_SLOT_SIZE,
        )
        .is_err()
    {
        stream.write_all(b"ERR flash erase\n").await?;
        return Ok(());
    }
    stream.write_all(b"READY\n").await?;
    stream.flush().await?;

    let mut hasher = Sha256::new();
    let mut received = 0u32;
    let mut next_progress = PROGRESS_INTERVAL;
    let mut buffer = [0xff; TRANSFER_BUFFER_SIZE];

    while received < header.image_length {
        let count = min(buffer.len(), (header.image_length - received) as usize);
        buffer.fill(0xff);
        read_exact(stream, &mut buffer[..count]).await?;

        if received == 0 && !has_mcuboot_image_magic(&buffer[..count]) {
            stream.write_all(b"ERR not an MCUboot image\n").await?;
            return Ok(());
        }

        hasher.update(&buffer[..count]);
        let padded = count.div_ceil(FLASH_WRITE_SIZE) * FLASH_WRITE_SIZE;
        if flash
            .blocking_write(STAGED_IMAGE_OFFSET + received, &buffer[..padded])
            .is_err()
        {
            stream.write_all(b"ERR flash write\n").await?;
            return Ok(());
        }
        received += count as u32;

        if received >= next_progress || received == header.image_length {
            write_progress(stream, received, header.image_length).await?;
            next_progress = received.saturating_add(PROGRESS_INTERVAL);
        }
    }

    if hasher.finalize().as_slice() != header.sha256 {
        stream.write_all(b"ERR transfer digest\n").await?;
        return Ok(());
    }

    // Re-read flash instead of trusting successful programming calls. This
    // catches address, programming, or memory-integrity errors before reboot.
    let mut readback_hasher = Sha256::new();
    let mut checked = 0u32;
    while checked < header.image_length {
        let count = min(buffer.len(), (header.image_length - checked) as usize);
        if flash
            .blocking_read(STAGED_IMAGE_OFFSET + checked, &mut buffer[..count])
            .is_err()
        {
            stream.write_all(b"ERR flash read-back\n").await?;
            return Ok(());
        }
        readback_hasher.update(&buffer[..count]);
        checked += count as u32;
    }
    if readback_hasher.finalize().as_slice() != header.sha256 {
        stream.write_all(b"ERR read-back digest\n").await?;
        return Ok(());
    }

    // The swap-info word is harmless without the magic. Program the magic
    // last so a reset at every earlier instruction keeps the old image active.
    let swap_info = trailer_swap_info_offset(SECONDARY_SLOT_OFFSET, SECONDARY_SLOT_SIZE);
    let magic = trailer_magic_block_offset(SECONDARY_SLOT_OFFSET, SECONDARY_SLOT_SIZE);
    if flash
        .blocking_write(swap_info, &mcuboot_test_swap_info_block())
        .is_err()
        || flash.blocking_write(magic, &mcuboot_magic_block()).is_err()
    {
        stream.write_all(b"ERR activation marker\n").await?;
        return Ok(());
    }

    stream.write_all(b"OK rebooting into trial image\n").await?;
    stream.flush().await?;
    Timer::after(Duration::from_millis(500)).await;
    cortex_m::peripheral::SCB::sys_reset()
}

async fn read_exact(
    input: &mut impl Read<Error = sunset::Error>,
    mut destination: &mut [u8],
) -> sunset::Result<()> {
    while !destination.is_empty() {
        let count = input.read(destination).await?;
        if count == 0 {
            return Err(sunset::Error::msg("firmware upload ended early"));
        }
        destination = &mut destination[count..];
    }
    Ok(())
}

async fn write_progress(
    output: &mut impl Write<Error = sunset::Error>,
    received: u32,
    total: u32,
) -> sunset::Result<()> {
    let mut line = String::<48>::new();
    let _ = writeln!(line, "PROGRESS {received}/{total}");
    output.write_all(line.as_bytes()).await?;
    output.flush().await
}
