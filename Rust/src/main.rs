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
mod network;
mod ssh_server;
mod udp_echo;

use core::cell::RefCell;

use critical_section::Mutex;
use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_stm32::eth::{Ethernet, GenericPhy, PacketQueue, Sma};
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::peripherals::{ETH, ETH_SMA, RNG};
use embassy_stm32::rng::Rng;
use embassy_stm32::{Config, bind_interrupts, eth, rng};
use nucleo_h723zg_udp_echo::MAC_ADDRESS;
use static_cell::StaticCell;
// Importing these crates as `_` keeps their symbols linked even though we do
// not call them directly. `defmt-rtt` transports logs through the debugger,
// and `panic-probe` reports panics using that logging channel.
use {defmt_rtt as _, panic_probe as _};

// This macro connects the STM32 `ETH` interrupt to Embassy's Ethernet driver
// at compile time. `Irqs` is a zero-sized proof that the correct handler was
// installed; it is passed to `Ethernet::new` later.
bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
    RNG => rng::InterruptHandler<RNG>;
});

// The complete generic Ethernet type is long, so this alias makes the task
// signature readable. `'static` means the device and PHY management interface
// remain valid for the whole firmware run.
type EthernetDevice = Ethernet<'static, ETH, GenericPhy<Sma<'static, ETH_SMA>>>;

// Async tasks and DMA cannot borrow temporary memory that disappears. A
// `StaticCell` safely initializes static memory exactly once, without a heap.
// The packet queue contains four receive and four transmit DMA descriptors.
static PACKET_QUEUE: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
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
async fn net_task(runner: embassy_net::Runner<'static, EthernetDevice>) -> ! {
    // `run` needs mutable access because it updates protocol and socket state.
    let mut runner = runner;
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    // Start with Embassy's safe reset defaults, then describe the desired
    // clock tree. The PLL calculation is:
    // 64 MHz HSI / 4 * 50 / 2 = 400 MHz system clock.
    // AHB runs at 200 MHz and each APB bus at 100 MHz.
    let mut config = Config::default();
    {
        // The RCC names are only needed in this block, so the wildcard import
        // is deliberately kept local instead of polluting the whole module.
        use embassy_stm32::rcc::*;

        config.rcc.hsi = Some(HSIPrescaler::DIV1);
        config.rcc.csi = true;
        config.rcc.pll1 = Some(Pll {
            source: PllSource::HSI,
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL50,
            divp: Some(PllDiv::DIV2),
            divq: None,
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV2;
        config.rcc.apb1_pre = APBPrescaler::DIV2;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.apb3_pre = APBPrescaler::DIV2;
        config.rcc.apb4_pre = APBPrescaler::DIV2;
        config.rcc.voltage_scale = VoltageScale::Scale1;
    }

    // Initialization consumes the configuration and returns ownership tokens
    // for every peripheral and pin. Rust then prevents two drivers from
    // accidentally controlling the same piece of hardware.
    let peripherals = embassy_stm32::init(config);
    info!("Rust NUCLEO-H723ZG UDP echo and SSH server booting");

    // HSI48 is enabled by Embassy's default H7 clock configuration and feeds
    // the hardware RNG. Install the driver before Sunset can request random
    // padding, ephemeral keys, or key-exchange nonces.
    let hardware_rng = Rng::new(peripherals.RNG, Irqs);
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
    let ethernet = Ethernet::new(
        PACKET_QUEUE.init(PacketQueue::new()),
        peripherals.ETH,
        Irqs,
        peripherals.PA1,
        peripherals.PA7,
        peripherals.PC4,
        peripherals.PC5,
        peripherals.PG13,
        peripherals.PB13,
        peripherals.PG11,
        MAC_ADDRESS,
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
    spawner.spawn(unwrap!(ssh_server::run(stack)));

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
