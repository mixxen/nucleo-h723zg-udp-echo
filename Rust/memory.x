/*
 * MCUboot-managed STM32H723ZG application layout.
 *
 * AXI SRAM is DMA-accessible; DTCM at 0x20000000 is intentionally not used
 * because the Ethernet DMA cannot access it.
 *
 * The primary slot starts at 0x08020000. Its first 0x200 bytes belong to the
 * MCUboot image header, so the Rust vector table begins at 0x08020200.
 * Offset-swap metadata owns the primary slot's final 128 KiB erase sector.
 * The signed image must therefore end before offset 0x40000. Limiting the
 * linked body to 253 KiB leaves 1024 bytes in that usable area for the header
 * and signature TLVs. The signing script performs the final size check.
 */
MEMORY
{
    FLASH : ORIGIN = 0x08020200, LENGTH = 253K
    RAM   : ORIGIN = 0x24000000, LENGTH = 128K
}
