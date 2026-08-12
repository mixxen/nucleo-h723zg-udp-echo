//! Native STM32 Ethernet MAC and LAN8742A-compatible RMII PHY bring-up.
//!
//! This file is the native device-specific portion of the trade study. It
//! stops at an Embassy network device; DHCP supervision and UDP serving live
//! in separate shared modules.

use embassy_stm32::Peri;
use embassy_stm32::bind_interrupts;
use embassy_stm32::eth::{self, Ethernet, GenericPhy, PacketQueue, Sma};
use embassy_stm32::peripherals::{ETH, ETH_SMA, PA1, PA2, PA7, PB13, PC1, PC4, PC5, PG11, PG13};
use nucleo_h723zg_udp_echo::MAC_ADDRESS;
use static_cell::StaticCell;

bind_interrupts!(struct EthernetInterrupts {
    ETH => eth::InterruptHandler;
});

pub type Device = Ethernet<'static, ETH, GenericPhy<Sma<'static, ETH_SMA>>>;

static PACKET_QUEUE: StaticCell<PacketQueue<4, 4>> = StaticCell::new();

/// Construct the native Ethernet device from the board's fixed RMII wiring.
#[allow(clippy::too_many_arguments)]
pub fn new(
    eth: Peri<'static, ETH>,
    ref_clk: Peri<'static, PA1>,
    crs_dv: Peri<'static, PA7>,
    rxd0: Peri<'static, PC4>,
    rxd1: Peri<'static, PC5>,
    txd0: Peri<'static, PG13>,
    txd1: Peri<'static, PB13>,
    tx_en: Peri<'static, PG11>,
    eth_sma: Peri<'static, ETH_SMA>,
    mdio: Peri<'static, PA2>,
    mdc: Peri<'static, PC1>,
) -> Device {
    Ethernet::new(
        PACKET_QUEUE.init(PacketQueue::new()),
        eth,
        EthernetInterrupts,
        ref_clk,
        crs_dv,
        rxd0,
        rxd1,
        txd0,
        txd1,
        tx_en,
        MAC_ADDRESS,
        eth_sma,
        mdio,
        mdc,
    )
}
