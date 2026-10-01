"""Fixed-fixture Rust codec batches and a reproducible source/dependency inventory."""

import argparse
from collections import defaultdict
import copy
import json
from pathlib import Path
import random
import subprocess

from compare import REPO, digest, provenance
from comparison_stats import percentile, write_csv


def inputs():
    cases = json.loads((REPO / "Rust/protocol/fixtures/cases.json").read_text())[
        "valid"
    ]
    selected = [
        case
        for case in cases
        if case["name"] not in ("device_info_json", "device_info_protobuf")
    ]
    result = []
    for format, encoding in (("csv", 1), ("json", 2), ("protobuf", 3)):
        for crc in ("off", "on"):
            for case in selected:
                message = copy.deepcopy(case["message"])
                if "device_info" in message:
                    message["device_info"]["encoding"] = encoding
                result.append(
                    dict(
                        format=format,
                        crc=crc,
                        fixture=case["name"].replace("_csv", ""),
                        message=message,
                    )
                )
    return result


def inventory():
    """Physical lines, including comments/blanks. Shared code is not charged per format."""
    sources = []
    for root in (
        "Rust/messaging/src",
        "Rust/tools/message-demo/src",
        "Rust/tools/message-demo/python",
    ):
        for path in sorted((REPO / root).rglob("*")):
            if path.suffix not in (".rs", ".py"):
                continue
            sources.append(
                dict(
                    path=str(path.relative_to(REPO)),
                    category=(
                        "generated" if "generated" in path.parts else "handwritten"
                    ),
                    physical_lines=len(path.read_text().splitlines()),
                )
            )
    for root in (
        "Rust/messaging",
        "Rust/tools/message-demo",
        "Rust/tools/message-demo/generator",
    ):
        import tomllib

        manifest = tomllib.loads((REPO / root / "Cargo.toml").read_text())
        for name, spec in manifest["dependencies"].items():
            yield dict(
                record="direct_dependency",
                path=root + "/Cargo.toml",
                name=name,
                specification=json.dumps(spec, sort_keys=True),
            )
    for row in sources:
        yield dict(record="source_file", **row)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", type=Path, required=True, help="release message-demo executable"
    )
    parser.add_argument("--output", type=Path, required=True, help="new directory")
    parser.add_argument("--iterations", type=int, default=10000)
    parser.add_argument("--batches", type=int, default=9)
    parser.add_argument("--seed", type=int, default=723)
    parser.add_argument(
        "--build-description",
        required=True,
        help="exact compiler, target, profile and flags",
    )
    args = parser.parse_args()
    if not 1 <= args.iterations <= 1_000_000 or not 1 <= args.batches <= 100:
        parser.error("iterations must be 1..1000000; batches 1..100")
    args.output.mkdir(parents=True, exist_ok=False)
    fixtures = inputs()
    random.Random(args.seed).shuffle(fixtures)
    fixture_text = "".join(
        json.dumps(fixture, separators=(",", ":")) + "\n" for fixture in fixtures
    )
    (args.output / "inputs.jsonl").write_text(fixture_text)
    metadata = dict(
        environment=provenance(),
        binary_sha256=digest(args.binary),
        build_description=args.build_description,
        seed=args.seed,
        iterations=args.iterations,
        batches=args.batches,
        scope="host in-process codec including validation, normalization/conversion, CRC; no UDP or logging",
        statistic="ns_per_operation is a batch mean; percentiles are across batch means, not individual calls",
        complete=False,
    )
    meta_path = args.output / "manifest.json"
    meta_path.write_text(json.dumps(metadata, indent=2) + "\n")
    result = subprocess.run(
        [str(args.binary.resolve()), "bench", str(args.iterations), str(args.batches)],
        cwd=REPO,
        input=fixture_text,
        text=True,
        check=True,
        capture_output=True,
    )
    (args.output / "batches.jsonl").write_text(result.stdout)
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    if len(rows) != len(fixtures) * args.batches * 2:
        raise ValueError("benchmark returned an incomplete batch set")
    groups = defaultdict(list)
    sizes = {}
    for row in rows:
        key = (row["format"], row["crc"], row["fixture"], row["operation"])
        groups[key].append(row["ns_per_operation"])
        sizes[key[:3]] = {
            field: row[field]
            for field in (
                "format",
                "crc",
                "fixture",
                "kind",
                "body_bytes",
                "datagram_bytes",
            )
        }
    summary = []
    for (format, crc, fixture, operation), values in sorted(groups.items()):
        summary.append(
            dict(
                format=format,
                crc=crc,
                fixture=fixture,
                operation=operation,
                batches=len(values),
                iterations_per_batch=args.iterations,
                batch_mean_p50_ns=percentile(values, 0.5),
                batch_mean_p95_ns=percentile(values, 0.95),
                batch_mean_min_ns=min(values),
                batch_mean_max_ns=max(values),
            )
        )
    write_csv(args.output / "codec.csv", summary)
    write_csv(args.output / "fixture_sizes.csv", list(sizes.values()))
    write_csv(args.output / "inventory.csv", list(inventory()))
    metadata["complete"] = True
    metadata["inputs_sha256"] = digest(args.output / "inputs.jsonl")
    metadata["batches_sha256"] = digest(args.output / "batches.jsonl")
    meta_path.write_text(json.dumps(metadata, indent=2) + "\n")
    print(
        f"{len(fixtures)} fixed fixtures/configurations, {len(rows)} timed batches; {args.output}"
    )


if __name__ == "__main__":
    main()
