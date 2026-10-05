# B1 benchmark contract

Status: B1 specification and reference checks. Host/firmware implementation is
scheduled for B2/B3. Date: 2026-10-05 UTC.

Authoritative definitions: [benchmark.proto](../../protocol/benchmark/benchmark.proto)
and [profile.json](../../protocol/benchmark/profile.json). Reviewed examples and
state scenarios: [fixtures.json](../../protocol/benchmark/fixtures.json).
See [resource budget](BENCHMARK_RESOURCES.md) and [phased plan](BENCHMARK_PLAN.md).

## Compatibility and integration

Use a separate `messaging.benchmark.v1` schema and opt-in `messaging-benchmark`
firmware feature, requiring the full messaging application and its single selected
encoding/CRC mode. The feature name is reserved here, not implemented in B1.
Normal messaging schema, command behavior, ports and default schedules stay
unchanged. Benchmark streams are additional streams; a run measures their load
alongside the ordinary application's 10 Hz status and 1 Hz health publications.
Record that background workload explicitly in comparisons.

Do not grow the normal `messaging.v1.Envelope`. A benchmark envelope owns only
one bounded synthetic payload, and its body should be a Rust enum, avoiding one
large allocation per possible body variant. Reuse the existing status/health
types, synthetic sample creation, validation, CSV parsing/formatting primitives,
JSON strictness rules, micropb generation and CRC framing. Extend the generators
for the separate namespace/model in B2; do not create a different toy serializer
and label its timings as the application codec path.

| Traffic | Port | Behavior |
|---|---:|---|
| Existing application commands | 42000 | Unchanged |
| Existing status/health multicast | 42001 / 42002 | Unchanged |
| Benchmark discovery/control/probes | 42010 | Unicast, one controlling peer socket |
| Benchmark status/health multicast | 42011 / 42012 | Group 239.255.42.1, TTL 1; source ports equal destination ports |
| Optional profiler | 5001 | Existing instrumented-image endpoint |

All benchmark ports are fixed for the initial board implementation. Listeners
join explicitly selected interfaces; no multicast acknowledgments or automatic
probe retries. Configuration/renewal/stop may be retried with the same identity
and contents under the idempotency rules below. Traffic control is not
authentication; use an isolated bench network.

Old firmware does not bind 42010 and is not presumed compatible. Preflight must
receive a matching capabilities response in the explicitly chosen encoding/CRC
mode. Silence means no valid response observed, not proof of old firmware. No
fallback to raw echo, other formats, or CRC-off is permitted. Unknown benchmark
protocol versions are rejected; normal protocol version 1 is unaffected.

## Framing and representation

One envelope per UDP datagram. Maximum encoded body: 1,016 bytes. CRC-off maximum:
1,016 bytes. CRC-on appends the existing eight uppercase ASCII CRC-32C hex digits
for a maximum of 1,024 bytes. Coverage and parameters are unchanged from
[Phase 5](PHASE5.md). Check size before CRC and CRC before decoding. No sync word.

Protobuf uses the schema's tags and normal Protobuf unknown-field/duplicate-field
semantics, followed by shared semantic validation. JSON is `benchmark-json-v1`,
not ProtoJSON: numeric enum values, snake_case keys, nested body, omitted absent
values, and rejection of null, unknown keys, duplicate keys and invalid controls.
CSV is a single CRLF-terminated record with the frozen common/body columns in the
profile; apply the existing strict quote, canonical integer and boolean rules.

CSV needs one explicit presence exception: for data kinds 5, 15, 21 and 22, the
payload column is present even when empty. For other kinds, an empty payload
column means absent and nonempty is invalid. JSON/Protobuf data messages must
include payload presence even at zero length. This is not a change to normal CSV.

## Envelope and message kinds

Every message requires protocol version 1, kind, device ID and session ID.
Device IDs use the existing 1..32 ASCII alphanumeric/underscore/dot/hyphen rule.
Boot and session IDs use 16 lowercase hex characters, not all zero. Sessions are
fresh client-generated run identities; boot IDs come from the existing board
boot nonce. The server epoch is a nonzero u32 generation independent of session.

| Kind | Body | Required metadata beyond the common identity |
|---|---|---|
| 1 capabilities | Empty | Nonzero request ID; boot/epoch/uptime/sequence/payload absent |
| 2 configure | Configuration | Current boot, current epoch, nonzero control request ID |
| 3 renew | Empty | Current boot, active epoch, nonzero control request ID |
| 4 stop | Empty | Current boot, active epoch, nonzero control request ID |
| 5 probe | Empty | Current boot, active epoch, nonzero probe request ID, payload |
| 11 capabilities response | Limits | Current boot/epoch, echoed session/request, uptime |
| 12 control response | ControlResult | Current boot, request epoch, echoed session/request, uptime |
| 15 probe response | Empty | Current boot, request epoch, echoed session/request/payload, uptime |
| 21 status publication | Existing Status | Current boot, active epoch/session, sequence, uptime, payload |
| 22 health publication | Existing Health | Current boot, active epoch/session, sequence, uptime, payload |

Requests omit uptime and sequence. Responses omit sequence. Publications omit
request ID. Only data kinds have payload. Exactly one body matching kind is
present. Imported status/health field-presence, enum, synthetic/validity and
temperature rules remain the same as the normal contract.

Control and probe request IDs use separate counters. Match responses using
peer/device/boot/session/epoch/request ID and expected response kind. Capabilities
requests have a separate correlation namespace and do not advance control state.
All responses use the server's current boot ID. Wrong-target or wrong-boot
requests are silently rejected; perform new discovery after detecting a reboot.

## Payload, periods and host limits

Payload length is application pattern bytes, not total datagram length. Use
ASCII alphabet `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz`.
Byte i is alphabet index `(seed % 62 + token % 62 + i) % 62`, where token is the
probe request ID or publication sequence. Compute reductions before addition
so overflow behavior is independent of language. Seed is a u32, including zero.
For example seed=0/token=0 begins `01234`, seed=61/token=0 begins `z012`, and
seed=token=4294967295 begins `6789`.

All probes/publications in a run use its configured 0..512-byte length and seed.
The probe responder validates the pattern, then echoes those bytes. Listeners
validate against the captured run configuration; unsolicited sessions do not
enter results. A wrong pattern or length is rejected and counted without a
probe reply. The pattern is a reproducible test payload, not authentication or a
replacement for CRC. Actual encoded lengths must be reported for each format.

| Control | Frozen B1 limit/default |
|---|---|
| Benchmark status period | 0 disables; otherwise 1,000..10,000,000 us; default 100,000 us |
| Benchmark health period | 0 disables; otherwise 10,000..10,000,000 us; default 1,000,000 us |
| Lease | 2,000..30,000 ms; default 10,000 ms |
| Host probe rate | Integer 0..10,000 Hz; 0 disables; default 10 Hz |
| Outstanding probes | 1..4,096; default 64 |
| Response deadline | 1..60,000 ms; default 1,000 ms |
| Host receive work per loop | At most 64 datagrams before checking pacing/timers |
| Reordering history | At most 4,096 sequence positions per stream |
| Default reporting interval | 1,000 ms; chosen duration/interval must be positive and finite |

CLI stream rates convert to `ceil(1,000,000 / requested_hz)` microseconds and
must fit the period limits. Log both requested Hz and applied period/effective Hz;
do not silently clamp. Fractional stream rates down to 0.1 Hz are supported.
Host rate/concurrency/deadline are generator settings, not board guarantees.
At high load choose a window appropriate to rate times deadline. If the window
fills, skip planned slots and label the interval host/window limited; never
claim the board sustained an offered rate the host did not send.

## Configuration and lease state

The server starts idle at epoch 1, with both benchmark streams off and no owner.
Capabilities is read-only. `available` means idle and epoch not exhausted;
discovery during a run returns false. Capabilities advertise the profile limits,
selected encoding and CRC mode. All Configuration fields are mandatory; a
configure applies all values atomically or none.

1. **Acquire:** a valid configure naming the current boot/epoch acquires an idle
   server. Bind owner to source IP/port plus session ID. Remember the configuration
   and control ID, reset benchmark counters, and start each enabled schedule.
2. **Active:** only owner probes are serviced. The first publication is due one
   period after configure, with sequence 0. Status and health have independent
   absolute schedules. Advance sequences for missed opportunities and send
   failures, but send at most the newest due sample. Sequence/uptime wrap at u32;
   the host's modular ordering must handle this within its bounded window.
3. **Renew:** an owner's fresh, increasing control ID extends expiry by the
   original lease duration from acceptance time. Do not change schedules,
   configuration or counters. Host renewal target is no later than one-third of
   the lease duration; probes and capabilities never renew it.
4. **End:** valid stop, expiry, link/configuration loss, or terminal benchmark
   task failure cancels benchmark publication, clears its queues and ownership,
   and advances epoch. Normal application behavior continues. Reconnect remains
   idle and requires a new configure; no automatic load resumption.

Check expiry **before** processing a datagram, including at exact equality.
Already handed-off datagrams may arrive after termination; listeners/correlation
must filter them. Queue clearing and cancellation are local actions, not proof
that an Ethernet frame was withdrawn. B3 must measure expiry responsiveness
under load, with explicit cooperative yields and bounded receive/send work.

Epoch never wraps within a boot. The final epoch may run once; ending it sets
an exhausted flag, leaving capabilities available=false and rejecting configure
with RESOURCE_EXHAUSTED until reboot. Thus an arbitrarily delayed configure from
an earlier completed run cannot restart it, even after many subsequent runs.

## Idempotency, failures and replies

Keep the last accepted control ID and exact control contents, not a potentially
colliding hash. An identical duplicate of that control while its run is active
returns ALREADY_APPLIED with a fresh remaining-lease snapshot. It never renews
the lease, resets counters, restarts a schedule or changes configuration.
Same ID with different contents returns REQUEST_CONFLICT. Smaller IDs return
STALE_CONTROL. A new configure while active returns BUSY, even from the owner.
To change sizes/rates, stop, discover the new epoch, and use a new session.

Other peers/sessions receive BUSY for otherwise valid controls in the active
epoch; their probes receive no reply. A non-current epoch returns STALE_RUN for
controls. Renew/stop in an idle current epoch return NOT_ACTIVE. After a completed
stop, a duplicate old stop returns STALE_RUN; this confirms an obsolete run but
must not be interpreted as a newly executed stop. No terminal replay cache is
needed. IDs never wrap within a session; reserve an ID for stop or let expiry
end a session whose control ID space is exhausted.

Every structurally valid control is assessed in this order: boot/target,
epoch/exhaustion, ownership, duplicate/conflicting/stale ID, configuration bounds,
then action. Invalid configuration does not acquire a lease or advance the
accepted-control watermark. Failed controls leave active settings unchanged.
Successful configure and renew return APPLIED; successful stop returns STOPPED.
ControlResult carries the code and remaining lease milliseconds at response
generation (zero when no matching active run exists). Capabilities/control
replies share a bounded bucket: burst 10, refill 10 per second. Valid stop/renew
actions still execute if their reply is suppressed. Probe replies bypass that
bucket to exercise load; there are no error replies to invalid probes.

Do not reply to CRC/decoding/version failures, malformed correlation, unknown
kinds, responses/publications sent to the command port, wrong target or wrong
boot. Structural field violations are diagnostic rejections. Validly structured
but out-of-range configuration may receive INVALID_CONFIGURATION. Error/reply
loss is possible; a client does not assume a mutation failed merely because an
ACK was not observed. Stop/lease expiry bounds cleanup after uncertainty.

## Counters and evidence

Benchmark health uses the existing Health shape with **run-scoped benchmark**
counters, separate from normal application counters. Clear them only on accepted
acquisition. `received_requests` counts accepted configure/renew/stop actions and
valid probes, once per accepted control ID; discovery/duplicates do not count.
`rejected_datagrams` counts rejected benchmark input while active, including
non-owner input. `send_errors` counts local failures; `skipped_publications`
counts missed benchmark publication opportunities, including local send failure.
Use saturating u32 counters and the same OK/DEGRADED service-health rules. Keep
separate internal CRC, per-stream send/skip and control-response-suppression
diagnostics; do not add fields to normal Health just for this benchmark.

Final counter snapshots may be unavailable after stop/reset/link loss. Host
results must preserve that gap and never fabricate a final sample. Local send
acceptance is not proof of physical transmission. Probe duplicates can legitimately
produce multiple replies; the host counts them, never as additional successes.
RTT uses host monotonic time immediately before send through validated response
correlation, including response decoding/pattern verification but excluding
request encoding. Multicast gets arrival/freshness metrics, not one-way latency.

Host statistical methods, bounded histogram precision and incremental file
formats are B2 work. Cross-machine, soak, reset and lease behavior remain B3–B5
runtime/physical gates; B1 fixtures do not demonstrate them.
