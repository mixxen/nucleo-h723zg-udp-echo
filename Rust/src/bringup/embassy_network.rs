//! Shared Embassy link and IPv4 configuration supervision.
//!
//! This task waits asynchronously rather than polling in a busy loop. While it
//! is waiting, Embassy can run the packet driver and UDP server tasks.

use defmt::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_net::{ConfigV4, Ipv4Address, Ipv4Cidr, Stack, StaticConfigV4};
use embassy_stm32::gpio::Output;
use embassy_time::{Duration, Timer};
// `heapless::Vec` has fixed capacity and performs no heap allocation, making
// its memory use predictable on a microcontroller.
use heapless::Vec;
use nucleo_h723zg_udp_echo::{FALLBACK_ADDRESS, FALLBACK_GATEWAY, FALLBACK_PREFIX_LENGTH};

// Give a DHCP server 30 seconds to assign an address before using the fallback.
const DHCP_TIMEOUT: Duration = Duration::from_secs(30);

#[embassy_executor::task]
pub async fn supervise(
    // `Stack` is a small, copyable handle to the shared Embassy network stack.
    // Its data lives in the static resources initialized in `main`.
    stack: Stack<'static>,
    // These values own their GPIO pins. `mut` is required because changing an
    // output level changes the driver's state.
    mut ready_led: Output<'static>,
    mut error_led: Output<'static>,
) -> ! {
    // The outer loop also handles unplugging and reconnecting the cable.
    loop {
        // Indicate that the interface is not ready to serve traffic yet.
        ready_led.set_low();
        error_led.set_high();

        info!("waiting for Ethernet link");
        // `.await` suspends this task until the PHY reports a link. It does not
        // block the processor or the other async tasks.
        stack.wait_link_up().await;
        info!("Ethernet link is up; waiting for DHCP");

        // `select` races two futures and returns whichever completes first:
        // either DHCP produces a usable configuration, or the timer expires.
        match select(stack.wait_config_up(), Timer::after(DHCP_TIMEOUT)).await {
            Either::First(()) => {
                // `config_v4` returns `Option` because a stack is allowed to
                // have no IPv4 configuration. `if let Some` safely unwraps it
                // only in the configured case.
                if let Some(config) = stack.config_v4() {
                    info!(
                        "DHCP address: {}/{} gateway: {:?}",
                        config.address.address(),
                        config.address.prefix_len(),
                        config.gateway
                    );
                }
            }
            Either::Second(()) => {
                warn!("DHCP timed out; using 192.168.0.10/24");
                // `Ipv4Cidr` stores both an address and its prefix length.
                // A /24 prefix corresponds to netmask 255.255.255.0.
                stack.set_config_v4(ConfigV4::Static(StaticConfigV4 {
                    address: Ipv4Cidr::new(
                        Ipv4Address::from(FALLBACK_ADDRESS),
                        FALLBACK_PREFIX_LENGTH,
                    ),
                    gateway: Some(Ipv4Address::from(FALLBACK_GATEWAY)),
                    // DNS is unnecessary for an echo server because it never
                    // resolves host names.
                    dns_servers: Vec::new(),
                }));
            }
        }

        // At this point either DHCP or the fallback supplied an address.
        ready_led.set_high();
        error_led.set_low();
        // Sleep until the PHY reports that the cable/link went away.
        stack.wait_link_down().await;
        warn!("Ethernet link is down");

        // Return to DHCP mode so a reconnect can obtain a fresh lease, then
        // repeat the loop from the link-waiting state.
        stack.set_config_v4(ConfigV4::Dhcp(Default::default()));
    }
}
