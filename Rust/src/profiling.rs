//! Low-overhead CPU and stack instrumentation for benchmark-only firmware.
//!
//! Embassy normally puts the MCU to sleep whenever every async task is
//! waiting. Its optional trace hooks tell us when the executor starts doing
//! work and when it becomes idle again. We time those busy intervals with the
//! Cortex-M7's DWT hardware cycle counter, then divide by wall-clock time on
//! the host. This measures executor work without putting a timer in the UDP
//! server's hot path.

use core::cell::RefCell;

use critical_section::Mutex;
use embassy_time::Instant;
use nucleo_h723zg_udp_echo::{PROFILING_MAGIC, PROFILING_WIRE_SIZE};

const CPU_HZ: u32 = 400_000_000;
const TIME_TICKS_HZ: u32 = 32_768;
const STACK_PAINT: u32 = 0xCCCC_CCCC;

struct State {
    busy_cycles: u64,
    polls: u64,
    active_since: Option<u32>,
    reset_ticks: u64,
}

static STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    busy_cycles: 0,
    polls: 0,
    active_since: None,
    reset_ticks: 0,
}));

pub fn init() {
    // DWT is a Cortex-M debug block, but its cycle counter also runs when no
    // debugger is attached. DCB trace must be enabled before CYCCNT can count.
    unsafe {
        let mut peripherals = cortex_m::Peripherals::steal();
        peripherals.DCB.enable_trace();
        peripherals.DWT.enable_cycle_counter();
    }
    reset();
}

pub fn reset() {
    let now = cortex_m::peripheral::DWT::cycle_count();
    critical_section::with(|cs| {
        *STATE.borrow(cs).borrow_mut() = State {
            busy_cycles: 0,
            polls: 0,
            active_since: Some(now),
            reset_ticks: Instant::now().as_ticks(),
        };
    });
}

pub fn encode_snapshot(output: &mut [u8; PROFILING_WIRE_SIZE]) {
    let now_cycles = cortex_m::peripheral::DWT::cycle_count();
    let now_ticks = Instant::now().as_ticks();
    let (busy_cycles, polls, reset_ticks) = critical_section::with(|cs| {
        let state = STATE.borrow(cs).borrow();
        let active = state
            .active_since
            .map_or(0, |start| now_cycles.wrapping_sub(start) as u64);
        (state.busy_cycles + active, state.polls, state.reset_ticks)
    });
    let (stack_used, stack_capacity) = stack_usage();

    output[..8].copy_from_slice(&PROFILING_MAGIC);
    put_u32(output, 8, CPU_HZ);
    put_u32(output, 12, TIME_TICKS_HZ);
    put_u64(output, 16, busy_cycles);
    put_u64(output, 24, now_ticks.saturating_sub(reset_ticks));
    put_u64(output, 32, polls);
    put_u32(output, 40, stack_used);
    put_u32(output, 44, stack_capacity);
}

fn stack_usage() -> (u32, u32) {
    unsafe extern "C" {
        static __sheap: u32;
        static _stack_start: u32;
    }

    // cortex-m-rt's `paint-stack` feature filled this unused area with a known
    // word at reset. The first overwritten word marks the stack high-water.
    let bottom = core::ptr::addr_of!(__sheap) as usize;
    let top = core::ptr::addr_of!(_stack_start) as usize;
    let mut cursor = bottom;
    while cursor < top {
        let word = unsafe { core::ptr::read_volatile(cursor as *const u32) };
        if word != STACK_PAINT {
            break;
        }
        cursor += core::mem::size_of::<u32>();
    }
    ((top - cursor) as u32, (top - bottom) as u32)
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[unsafe(no_mangle)]
fn _embassy_trace_poll_start(_executor_id: u32) {
    let now = cortex_m::peripheral::DWT::cycle_count();
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        // Under sustained load the executor may begin another poll without
        // first becoming idle. Close the preceding busy interval here so no
        // time is lost and the 32-bit DWT counter cannot wrap between samples.
        if let Some(start) = state.active_since {
            state.busy_cycles += now.wrapping_sub(start) as u64;
        }
        state.active_since = Some(now);
        state.polls += 1;
    });
}

#[unsafe(no_mangle)]
fn _embassy_trace_executor_idle(_executor_id: u32) {
    let now = cortex_m::peripheral::DWT::cycle_count();
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        if let Some(start) = state.active_since.take() {
            state.busy_cycles += now.wrapping_sub(start) as u64;
        }
    });
}

#[unsafe(no_mangle)]
fn _embassy_trace_task_new(_executor_id: u32, _task_id: u32) {}
#[unsafe(no_mangle)]
fn _embassy_trace_task_end(_executor_id: u32, _task_id: u32) {}
#[unsafe(no_mangle)]
fn _embassy_trace_task_exec_begin(_executor_id: u32, _task_id: u32) {}
#[unsafe(no_mangle)]
fn _embassy_trace_task_exec_end(_executor_id: u32, _task_id: u32) {}
#[unsafe(no_mangle)]
fn _embassy_trace_task_ready_begin(_executor_id: u32, _task_id: u32) {}
