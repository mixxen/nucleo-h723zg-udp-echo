/*
 * Conservative STM32H723ZG layout.
 *
 * AXI SRAM is DMA-accessible; DTCM at 0x20000000 is intentionally not used
 * because the Ethernet DMA cannot access it.
 */
MEMORY
{
    FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
    RAM   : ORIGIN = 0x24000000, LENGTH = 128K
}
