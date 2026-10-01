# Phase 6 retained evidence

See [the comparison report](../../COMPARISON.md) for interpretation and
[the walkthrough](../../PHASE6.md) for exact measurement definitions and commands.

- `codec.csv`: median/p95/min/max of batch means for 16 fixtures, six configurations
  and encode/decode. Nine batches of 10,000 operations each.
- `codec_batches.csv`: all 1,728 batch samples exported from the benchmark's JSONL.
  These allow recalculating `codec.csv` without rerunning the timing experiment.
- `fixture_sizes.csv`: exact Rust body/datagram sizes, including both CRC modes.
- `inventory.csv`: direct Cargo dependencies and selected source physical lines;
  generated and handwritten files are distinguished. This is not a human-effort
  estimate or a repository-wide count.
- `codec-provenance.json`: original benchmark manifest, source hashes, binary
  hash and environment. It identifies a pre-commit working tree on the Phase 5
  merge. Its source snapshot predates later comparison-only error/export fixes;
  the Rust benchmark and portable codec files are unchanged.
- `phase5_resources.json`: unmodified **prior** Phase 5 local build resource
  report, carrying its original `deb811c-dirty` revision marker. Do not treat it
  as a Phase 6 build or physical measurement. The firmware/codec sources and
  dependency lockfiles were not changed in Phase 6.

Traffic exports and provenance describe the separate 60-second/configuration
host preview. The full raw traffic capture is not checked into Git. Archive the
complete runner output for future bench runs; these compact CSVs cannot recreate
raw messages, all warm-up observations, or every event's timing. Log hashes are
retained for comparison with the original capture.

Checked-in CSV reports use LF line endings for clean Git diffs; values are
unchanged from the CSV exports. This does not change the CSV wire format, which
still requires CRLF.

None of these files demonstrates physical NUCLEO execution. Missing hardware
measurements stay pending in the report, not replaced with zeroes or host values.
