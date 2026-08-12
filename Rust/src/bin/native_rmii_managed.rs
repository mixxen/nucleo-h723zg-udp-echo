//! Board initialization and top-level task wiring.
//!
//! Embedded programs do not have an operating system to initialize hardware
//! or schedule threads for them. This file configures the MCU, constructs the
//! Ethernet/network objects, and starts four cooperative async tasks.

// `no_std` replaces Rust's normal standard library with `core`. The standard
// library expects an operating system, files, threads, and heap allocation,
// none of which are available on this bare-metal microcontroller.
#![no_std]
// There is no operating-system entry point, so Embassy supplies one through
// the `#[embassy_executor::main]` attribute below.
#![no_main]

// Each module owns one distinct part of the application.
#[path = "../board.rs"]
mod board;
#[path = "../firmware_update.rs"]
mod firmware_update;
#[path = "../bringup/native_rmii.rs"]
mod native_rmii;
#[path = "../bringup/embassy_network.rs"]
mod network;
#[path = "../ssh_server.rs"]
mod ssh_server;
#[path = "../servers/embassy_udp_echo.rs"]
mod udp_echo;

use core::cell::RefCell;

use critical_section::Mutex;
use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_net::StackResources;
use embassy_stm32::flash::Flash;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::RNG;
use embassy_stm32::rng::Rng;
use embassy_stm32::{bind_interrupts, rng};
use static_cell::StaticCell;
// Importing these crates as `_` keeps their symbols linked even though we do
// not call them directly. `defmt-rtt` transports logs through the debugger,
// and `panic-probe` reports panics using that logging channel.
use {defmt_rtt as _, panic_probe as _};

// This macro connects the STM32 `ETH` interrupt to Embassy's Ethernet driver
// at compile time. `Irqs` is a zero-sized proof that the correct handler was
// installed; it is passed to `Ethernet::new` later.
bind_interrupts!(struct RngInterrupts {
    RNG => rng::InterruptHandler<RNG>;
});
// `StackResources<4>` covers DHCP, UDP echo, SSH TCP, and one spare socket.
static NETWORK_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();

// Sunset obtains cryptographic randomness through the `getrandom` crate's
// custom embedded hook. The actual entropy comes from the STM32 hardware RNG,
// kept here after its peripheral ownership token is consumed.
static HARDWARE_RNG: Mutex<RefCell<Option<Rng<'static, RNG>>>> = Mutex::new(RefCell::new(None));

// Register our function as the entropy source for Sunset and its cryptographic
// dependencies on this otherwise unsupported bare-metal target.
getrandom::register_custom_getrandom!(hardware_random);

// This task continuously drives the network stack. The return type `!`
// ("never") documents that a successful network runner does not terminate.
#[embassy_executor::task]
async fn net_task(runner: embassy_net::Runner<'static, native_rmii::Device>) -> ! {
    // `run` needs mutable access because it updates protocol and socket state.
    let mut runner = runner;
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = board::init(false);

    info!("Rust NUCLEO-H723ZG UDP echo and SSH server booting");

    // Keep ownership of internal flash for trial confirmation and authenticated
    // updates. It is handed to the SSH task only after the startup health
    // checkpoint below.
    #[cfg_attr(feature = "rollback-test", allow(unused_mut))]
    let mut flash = Flash::new_blocking(peripherals.FLASH);

    // HSI48 is enabled by Embassy's default H7 clock configuration and feeds
    // the hardware RNG. Install the driver before Sunset can request random
    // padding, ephemeral keys, or key-exchange nonces.
    let hardware_rng = Rng::new(peripherals.RNG, RngInterrupts);
    critical_section::with(|section| {
        HARDWARE_RNG.borrow(section).replace(Some(hardware_rng));
    });

    // PE1 is the network-ready LED; PB14 is the error/waiting LED.
    let ready_led = Output::new(peripherals.PE1, Level::Low, Speed::Low);
    let error_led = Output::new(peripherals.PB14, Level::High, Speed::Low);

    // Construct the STM32 Ethernet MAC and the generic LAN8742-compatible PHY
    // driver. The RMII pins, in constructor order, are:
    // PA1 REF_CLK, PA7 CRS_DV, PC4 RXD0, PC5 RXD1,
    // PG13 TXD0, PB13 TXD1, and PG11 TX_EN.
    // PA2 (MDIO) and PC1 (MDC) form the management interface used to configure
    // and query the external PHY.
    let ethernet = native_rmii::new(
        peripherals.ETH,
        peripherals.PA1,
        peripherals.PA7,
        peripherals.PC4,
        peripherals.PC5,
        peripherals.PG13,
        peripherals.PB13,
        peripherals.PG11,
        peripherals.ETH_SMA,
        peripherals.PA2,
        peripherals.PC1,
    );

    // Embassy splits networking into two handles:
    // - `stack` is cheap to copy and is used by application tasks.
    // - `runner` owns the work loop that moves packets and advances timers.
    // DHCP is the initial IPv4 configuration. The final number is a random
    // seed used internally for protocol values such as ephemeral ports.
    let (stack, runner) = embassy_net::new(
        ethernet,
        embassy_net::Config::dhcpv4(Default::default()),
        NETWORK_RESOURCES.init(StackResources::new()),
        0x48_37_32_33_5a_47_00_01,
    );

    // A spawner schedules async tasks cooperatively on one executor. The task
    // macros return a `Result` because each task has a finite static storage
    // pool; `unwrap!` turns an impossible startup allocation failure into a
    // useful defmt panic instead of silently skipping a task.
    spawner.spawn(unwrap!(net_task(runner)));
    spawner.spawn(unwrap!(network::supervise(stack, ready_led, error_led)));
    spawner.spawn(unwrap!(udp_echo::run(stack)));

    // MCUboot leaves a one-byte "please confirm" flag erased during a trial
    // boot. Yield several executor turns so the Ethernet runner and link
    // supervisor are actually polled before declaring the image healthy.
    // Unlike a timed delay, this checkpoint does not depend on a cable, DHCP,
    // or even a working wall-clock timer. On a normal factory boot the trailer
    // has no MCUboot magic and this call changes nothing.
    for _ in 0..3 {
        yield_now().await;
    }
    #[cfg(not(feature = "rollback-test"))]
    firmware_update::confirm_running_trial(&mut flash);
    #[cfg(feature = "rollback-test")]
    defmt::warn!("rollback-test build: deliberately withholding MCUboot confirmation");

    spawner.spawn(unwrap!(ssh_server::run(stack, flash)));

    // All useful work now lives in spawned tasks. Keep `main` alive forever
    // without consuming CPU; the executor wakes other tasks on events/timers.
    core::future::pending().await
}

/// Supply Sunset/getrandom with bytes from the STM32 hardware RNG.
///
/// The critical section prevents two callers from mutably borrowing the
/// peripheral simultaneously; Embassy runs this firmware on one executor, so
/// the short synchronous hardware reads do not contend with another CPU thread.
fn hardware_random(destination: &mut [u8]) -> Result<(), getrandom::Error> {
    critical_section::with(|section| {
        let mut slot = HARDWARE_RNG.borrow(section).borrow_mut();
        let rng = slot.as_mut().ok_or(getrandom::Error::UNEXPECTED)?;
        rng.fill_bytes(destination);
        Ok(())
    })
}
