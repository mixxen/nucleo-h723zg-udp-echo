//! Link-only ARM smoke test: every codec is reachable, with no global allocator.
#![no_std]
#![no_main]
use core::hint::black_box;
use messaging_codec::framing::{CrcMode, MAX_DATAGRAM, decode as decode_frame, encode};
use messaging_codec::{Format, decode};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let input = black_box(br#"{"protocol_version":1,"kind":2,"device_id":"board-01","client_session":"0123456789abcdef","request_id":1,"get_status":{}}"#);
    let message = decode(Format::Json, input).unwrap();
    let mut bytes = [0; MAX_DATAGRAM];
    #[cfg(feature = "benchmark")]
    {
        use messaging_codec::benchmark;
        const _: () = assert!(core::mem::size_of::<benchmark::model::Envelope>() <= 1536);
        let input = black_box(br#"{"protocol_version":1,"kind":1,"device_id":"board-01","session_id":"0123456789abcdef","request_id":1,"capabilities":{}}"#);
        let message = benchmark::decode(Format::Json, CrcMode::Off, input).unwrap();
        for format in [Format::Csv, Format::Json, Format::Protobuf] {
            for crc in [CrcMode::Off, CrcMode::On] {
                let length =
                    benchmark::encode(black_box(format), black_box(crc), &message, &mut bytes)
                        .unwrap();
                black_box(benchmark::decode(format, crc, black_box(&bytes[..length])).unwrap());
            }
        }
    }
    for format in [Format::Csv, Format::Json, Format::Protobuf] {
        for crc in [CrcMode::Off, CrcMode::On] {
            let length = encode(black_box(format), black_box(crc), &message, &mut bytes).unwrap();
            black_box(decode_frame(format, crc, black_box(&bytes[..length])).unwrap());
        }
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
