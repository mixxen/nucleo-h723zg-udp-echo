//! Link-only ARM smoke test: every codec is reachable, with no global allocator.
#![no_std]
#![no_main]
use core::hint::black_box;
use messaging_codec::{Format, MAX_BODY, decode, encode};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let input = black_box(br#"{"protocol_version":1,"kind":2,"device_id":"board-01","client_session":"0123456789abcdef","request_id":1,"get_status":{}}"#);
    let message = decode(Format::Json, input).unwrap();
    let mut bytes = [0; MAX_BODY];
    for format in [Format::Csv, Format::Json, Format::Protobuf] {
        let length = encode(black_box(format), &message, &mut bytes).unwrap();
        black_box(decode(format, black_box(&bytes[..length])).unwrap());
    }
    loop {
        core::hint::spin_loop();
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
