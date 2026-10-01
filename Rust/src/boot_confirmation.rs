//! MCUboot confirmation without the SSH updater or its dependencies.
use embassy_stm32::flash::{Blocking, Flash};
use nucleo_h723zg_udp_echo::{
    MCUBOOT_TRAILER_MAGIC, PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE, mcuboot_image_ok_block,
    trailer_image_ok_offset, trailer_magic_block_offset,
};

#[derive(Debug, defmt::Format)]
pub enum ConfirmationError {
    Read,
    UnexpectedFlag,
    Write,
    Verify,
}
#[derive(Debug, defmt::Format)]
pub enum Confirmation {
    FactoryImage,
    AlreadyConfirmed,
    Confirmed,
}

/// Factory images have no trailer magic. Only program an erased trial flag;
/// read it back before reporting success. No erase or slot change occurs here.
pub fn confirm_running_trial(
    flash: &mut Flash<'static, Blocking>,
) -> Result<Confirmation, ConfirmationError> {
    let magic_offset = trailer_magic_block_offset(PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE);
    let mut magic = [0; 16];
    flash
        .blocking_read(magic_offset + 16, &mut magic)
        .map_err(|_| ConfirmationError::Read)?;
    if magic != MCUBOOT_TRAILER_MAGIC {
        return Ok(Confirmation::FactoryImage);
    }
    let offset = trailer_image_ok_offset(PRIMARY_SLOT_OFFSET, PRIMARY_SLOT_SIZE);
    let mut block = [0; 32];
    flash
        .blocking_read(offset, &mut block)
        .map_err(|_| ConfirmationError::Read)?;
    if block[0] == 1 {
        return Ok(Confirmation::AlreadyConfirmed);
    }
    if block != [0xff; 32] {
        return Err(ConfirmationError::UnexpectedFlag);
    }
    flash
        .blocking_write(offset, &mcuboot_image_ok_block())
        .map_err(|_| ConfirmationError::Write)?;
    flash
        .blocking_read(offset, &mut block)
        .map_err(|_| ConfirmationError::Read)?;
    if block != mcuboot_image_ok_block() {
        return Err(ConfirmationError::Verify);
    }
    Ok(Confirmation::Confirmed)
}
