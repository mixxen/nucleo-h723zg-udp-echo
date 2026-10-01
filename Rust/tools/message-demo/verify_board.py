"""Build/lint/sign each single-format NUCLEO image and report linked storage.

Requires the firmware stable toolchain with thumbv7em-none-eabihf, clippy,
llvm-tools, and imgtool 2.4.0 in this Python environment. Never flashes a board.
The temporary key is disposable and cannot sign for the provisioned bootloader.
"""

import itertools
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

RUST = Path(__file__).resolve().parents[2]
ARTIFACTS = RUST / "artifacts" / "messaging"
BINARY = "nucleo-h723zg-native-rmii-messaging"
TARGET = "thumbv7em-none-eabihf"


def run(args, **kwargs):
    print("+ " + " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), cwd=RUST, check=True, **kwargs)


def capture(args):
    return run(args, capture_output=True, text=True).stdout.strip()


def cargo(operation, features):
    return [
        "cargo",
        "+stable",
        operation,
        "--locked",
        "--release",
        "--target",
        TARGET,
        "--no-default-features",
        "--features",
        features,
        "--bin",
        BINARY,
    ]


def main():
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    host = next(
        line.split(": ", 1)[1]
        for line in capture(["rustc", "+stable", "-vV"]).splitlines()
        if line.startswith("host: ")
    )
    llvm = (
        Path(capture(["rustc", "+stable", "--print", "sysroot"]))
        / "lib"
        / "rustlib"
        / host
        / "bin"
    )
    imgtool = [sys.executable, "-m", "imgtool.main"]
    report = {
        "revision": capture(["git", "describe", "--always", "--dirty"]),
        "compiler": capture(["rustc", "+stable", "--version"]),
        "target": TARGET,
        "profile": "release",
        "hardware_tested": False,
        "images": [],
    }
    run(["cargo", "+stable", "fmt", "--check"])
    with tempfile.TemporaryDirectory(prefix="messaging-sign-") as temporary:
        key = Path(temporary) / "ci-ed25519.pem"
        run(imgtool + ["keygen", "--key", key, "--type", "ed25519"])
        for encoding, crc in itertools.product(
            ("csv", "json", "protobuf"), ("off", "on")
        ):
            label = f"{encoding}-{crc}"
            features = (
                "messaging-" + encoding + (",messaging-crc" if crc == "on" else "")
            )
            metadata = json.loads(
                capture(
                    [
                        "cargo",
                        "+stable",
                        "metadata",
                        "--locked",
                        "--format-version",
                        "1",
                        "--filter-platform",
                        TARGET,
                        "--no-default-features",
                        "--features",
                        features,
                    ]
                )
            )
            codec = next(
                p["id"] for p in metadata["packages"] if p["name"] == "messaging-codec"
            )
            enabled = next(
                n["features"] for n in metadata["resolve"]["nodes"] if n["id"] == codec
            )
            assert enabled == [encoding], f"Unexpected codec features: {enabled}"
            run(cargo("clippy", features) + ["--", "-D", "warnings"])
            run(cargo("build", features))
            elf = ARTIFACTS / f"{label}.elf"
            shutil.copyfile(RUST / "target" / TARGET / "release" / BINARY, elf)
            unsigned = ARTIFACTS / f"{label}-unsigned.bin"
            signed = ARTIFACTS / f"{label}-ci-signed.bin"
            run([llvm / "llvm-objcopy", "-O", "binary", elf, unsigned])
            run(
                imgtool
                + [
                    "sign",
                    "--key",
                    key,
                    "--align",
                    "32",
                    "--max-align",
                    "32",
                    "--header-size",
                    "0x200",
                    "--pad-header",
                    "--slot-size",
                    "0x60000",
                    "--max-sectors",
                    "4",
                    "--version",
                    "0.0.0+0",
                    unsigned,
                    signed,
                ]
            )
            run(imgtool + ["verify", "--key", key, signed])
            assert signed.stat().st_size <= 262144, "Signed image exceeds MCUboot limit"
            sections = capture([llvm / "llvm-size", "-A", elf])
            (ARTIFACTS / f"{label}-sections.txt").write_text(sections + "\n")
            ram = sum(
                int(size)
                for _, size, address in re.findall(
                    r"^(\S+)\s+(\d+)\s+(\d+)\s*$", sections, re.M
                )
                if 0x24000000 <= int(address) < 0x24020000
            )
            symbols = capture(
                [
                    llvm / "llvm-nm",
                    "--print-size",
                    "--size-sort",
                    "--radix=d",
                    "--demangle",
                    elf,
                ]
            )
            (ARTIFACTS / f"{label}-symbols.txt").write_text(symbols + "\n")
            report["images"].append(
                {
                    "encoding": encoding,
                    "crc": crc,
                    "codec_features": enabled,
                    "unsigned_bytes": unsigned.stat().st_size,
                    "signed_bytes": signed.stat().st_size,
                    "static_ram_bytes": ram,
                    "ram_remaining_for_stack_bytes": 131072 - ram,
                }
            )
    # Instrumented and deliberately unconfirmed builds stay outside the baseline size table.
    for extra in ("profiling", "rollback-test"):
        run(
            cargo("clippy", "messaging-protobuf,messaging-crc," + extra)
            + ["--", "-D", "warnings"]
        )
        run(cargo("build", "messaging-protobuf,messaging-crc," + extra))
    # An ambiguous firmware image must fail before it can be packaged.
    for invalid in ("messaging", "messaging-csv,messaging-json"):
        result = subprocess.run(
            cargo("check", invalid), cwd=RUST, capture_output=True, text=True
        )
        assert (
            result.returncode != 0 and "Select exactly one" in result.stderr
        ), result.stderr
    (ARTIFACTS / "resources.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    print(
        "ARM build/package gates passed. Physical boot/network/recovery gates remain separate."
    )


if __name__ == "__main__":
    main()
