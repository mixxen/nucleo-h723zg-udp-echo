//! Rebuild each comparison image with the same settings. Wire format is a feature.
use embassy_net::Ipv4Address;
use messaging_codec::Format;
pub const DEVICE_ID: &str = "board-01";
pub const FIRMWARE_VERSION: &str = "messaging-0.1.0";
pub const COMMAND_PORT: u16 = 42000;
pub const STATUS_PORT: u16 = 42001;
pub const HEALTH_PORT: u16 = 42002;
pub const GROUP: Ipv4Address = Ipv4Address::new(239, 255, 42, 1);
pub const STATUS_PERIOD_MS: u64 = 100;
pub const HEALTH_PERIOD_MS: u64 = 1000;
pub const DATAGRAM_CAPACITY: usize = 1024;

#[cfg(any(
    all(feature = "messaging-csv", feature = "messaging-json"),
    all(feature = "messaging-csv", feature = "messaging-protobuf"),
    all(feature = "messaging-json", feature = "messaging-protobuf"),
    not(any(
        feature = "messaging-csv",
        feature = "messaging-json",
        feature = "messaging-protobuf"
    ))
))]
compile_error!("Select exactly one of messaging-csv, messaging-json, messaging-protobuf");
#[cfg(any(
    feature = "firmware",
    feature = "native-udp",
    feature = "multicast-smoke",
    feature = "wiznet",
    feature = "wiznet-offload",
    feature = "performance"
))]
compile_error!(
    "Messaging requires --no-default-features and cannot mix application/performance variants"
);

pub const FORMAT: Format = if cfg!(feature = "messaging-csv") {
    Format::Csv
} else if cfg!(feature = "messaging-json") {
    Format::Json
} else {
    Format::Protobuf
};
const _: () = {
    assert!(COMMAND_PORT != 0 && STATUS_PORT != 0 && HEALTH_PORT != 0);
    assert!(
        COMMAND_PORT != STATUS_PORT && COMMAND_PORT != HEALTH_PORT && STATUS_PORT != HEALTH_PORT
    );
    assert!(COMMAND_PORT != 5001 && STATUS_PORT != 5001 && HEALTH_PORT != 5001);
    assert!(STATUS_PERIOD_MS > 0 && HEALTH_PERIOD_MS > 0);
};
