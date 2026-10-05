"""Run the portable messaging gates locally and in CI, from any working directory."""

import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
RUST = REPO / "Rust"
TOOLCHAIN = "+1.90.0"


def run(args, **kwargs):
    print("+ " + " ".join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), check=True, cwd=REPO, **kwargs)


host = next(
    line.split(": ", 1)[1]
    for line in subprocess.check_output(
        ["rustc", TOOLCHAIN, "-vV"], text=True
    ).splitlines()
    if line.startswith("host: ")
)
run([sys.executable, RUST / "protocol/tools/check_phase1.py"])
run([sys.executable, RUST / "protocol/benchmark/check_contract.py"])
run([sys.executable, HERE / "test_load_metrics.py"])
run([sys.executable, HERE / "check_generation.py"])
run([sys.executable, HERE / "generate_benchmark.py", "--check"])
for crate in (RUST / "messaging", HERE, HERE / "generator"):
    run(["cargo", TOOLCHAIN, "fmt", "--manifest-path", crate / "Cargo.toml", "--check"])
run(
    [
        "cargo",
        TOOLCHAIN,
        "clippy",
        "--locked",
        "--manifest-path",
        HERE / "Cargo.toml",
        "--target",
        host,
        "--",
        "-D",
        "warnings",
    ]
)
run(
    [
        "cargo",
        TOOLCHAIN,
        "clippy",
        "--locked",
        "--manifest-path",
        RUST / "messaging/Cargo.toml",
        "--target",
        "thumbv7em-none-eabihf",
        "--lib",
        "--",
        "-D",
        "warnings",
    ]
)
run(
    [
        "cargo",
        TOOLCHAIN,
        "build",
        "--locked",
        "--manifest-path",
        HERE / "Cargo.toml",
        "--target",
        host,
    ]
)
env = dict(
    os.environ,
    MESSAGE_DEMO_BIN=str(
        HERE
        / "target"
        / host
        / "debug"
        / ("message-demo.exe" if os.name == "nt" else "message-demo")
    ),
)
run(
    [
        "cargo",
        TOOLCHAIN,
        "test",
        "--locked",
        "--manifest-path",
        RUST / "messaging/Cargo.toml",
        "--target",
        host,
        "--lib",
    ]
)
run([sys.executable, HERE / "test_demo.py"], env=env)
run([sys.executable, HERE / "test_multicast.py"], env=env)
run([sys.executable, HERE / "test_crc.py"], env=env)
run([sys.executable, HERE / "test_compare.py"], env=env)
run([sys.executable, HERE / "test_benchmark_live.py"], env=env)
# Run from the repository root so the existing firmware's link.x/defmt.x configuration
# is not inherited. This is a link-only portable-codec probe, not a board image.
run(
    [
        "cargo",
        TOOLCHAIN,
        "rustc",
        "--locked",
        "--release",
        "--manifest-path",
        RUST / "messaging/Cargo.toml",
        "--target",
        "thumbv7em-none-eabihf",
        "--example",
        "no_alloc",
        "--features",
        "benchmark",
        "--",
        "-C",
        "link-arg=-e_start",
    ]
)
print(
    "Host, multicast, command policy, generation, lint, and ARM no-allocator link gates passed. Run verify_board.py separately for the six Phase 5 firmware configurations."
)
