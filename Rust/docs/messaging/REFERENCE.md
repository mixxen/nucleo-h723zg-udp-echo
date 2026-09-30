# Generated message reference

Source: `Rust/protocol/messaging.proto`. Application semantics remain in CONTRACT.md.

## Empty

| Field | Tag | Type |
|---|---:|---|

## DeviceInfo

| Field | Tag | Type |
|---|---:|---|
| firmware_version | 1 | heapless::String<32> |
| encoding | 2 | i32 |

## Status

| Field | Tag | Type |
|---|---:|---|
| temperature_celsius | 1 | f32 |
| validity | 2 | i32 |
| synthetic | 3 | bool |

## Health

| Field | Tag | Type |
|---|---:|---|
| state | 1 | i32 |
| received_requests | 2 | u32 |
| rejected_datagrams | 3 | u32 |
| send_errors | 4 | u32 |
| skipped_publications | 5 | u32 |

## Error

| Field | Tag | Type |
|---|---:|---|
| code | 1 | i32 |

## Envelope

| Field | Tag | Type |
|---|---:|---|
| protocol_version | 1 | u32 |
| kind | 2 | i32 |
| device_id | 3 | heapless::String<32> |
| boot_id | 4 | heapless::String<16> |
| client_session | 5 | heapless::String<16> |
| request_id | 6 | u32 |
| sequence | 7 | u32 |
| uptime_ms | 8 | u32 |
| get_device_info | 20 | Empty |
| get_status | 21 | Empty |
| get_health | 22 | Empty |
| device_info | 23 | DeviceInfo |
| status | 24 | Status |
| health | 25 | Health |
| error | 26 | Error |

## MessageKind

| Value | Number |
|---|---:|
| MESSAGE_KIND_UNSPECIFIED | 0 |
| GET_DEVICE_INFO | 1 |
| GET_STATUS | 2 |
| GET_HEALTH | 3 |
| DEVICE_INFO_RESPONSE | 11 |
| STATUS_RESPONSE | 12 |
| HEALTH_RESPONSE | 13 |
| ERROR_RESPONSE | 14 |
| STATUS_PUBLICATION | 21 |
| HEALTH_PUBLICATION | 22 |

## Encoding

| Value | Number |
|---|---:|
| ENCODING_UNSPECIFIED | 0 |
| CSV | 1 |
| JSON | 2 |
| PROTOBUF | 3 |

## Validity

| Value | Number |
|---|---:|
| VALIDITY_UNSPECIFIED | 0 |
| VALID | 1 |
| UNAVAILABLE | 2 |
| SENSOR_FAULT | 3 |

## HealthState

| Value | Number |
|---|---:|
| HEALTH_STATE_UNSPECIFIED | 0 |
| OK | 1 |
| DEGRADED | 2 |

## ErrorCode

| Value | Number |
|---|---:|
| ERROR_CODE_UNSPECIFIED | 0 |
| UNSUPPORTED_KIND | 1 |
| INVALID_REQUEST | 2 |
