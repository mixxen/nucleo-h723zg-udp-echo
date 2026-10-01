"""Sequence/freshness rules and two real multicast listeners with concurrent commands."""

import contextlib
import copy
import io
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from test_demo import HERE, BINARY, CASES, server
from client import Client, EventLog
from codec import FORMATS, encode
from listener import join_group, receive
from stream_state import DeviceStreams, StreamState, MASK, HALF

GROUP = "239.255.42.1"
INTERFACE = "127.0.0.1"


def events(path):
    return (
        [json.loads(line) for line in Path(path).read_text().split("\n")[:-1]]
        if Path(path).exists()
        else []
    )


def wait_until(predicate, timeout=5):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.01)
    raise AssertionError("condition did not become true before timeout")


def ports():
    # Reserve two distinct free ports while selecting them; release immediately before use.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as first, socket.socket(
        socket.AF_INET, socket.SOCK_DGRAM
    ) as second:
        first.bind((INTERFACE, 0))
        second.bind((INTERFACE, 0))
        return first.getsockname()[1], second.getsockname()[1]


def options(status, health):
    return (
        "--multicast-interface",
        INTERFACE,
        "--group",
        GROUP,
        "--status-port",
        str(status),
        "--health-port",
        str(health),
        "--status-hz",
        "20",
        "--health-hz",
        "5",
    )


@contextlib.contextmanager
def listener_process(format, status, health, path):
    with open(str(path) + ".stderr", "w") as error:
        process = subprocess.Popen(
            [
                sys.executable,
                str(HERE / "python/listener.py"),
                "--format",
                format,
                "--interface",
                INTERFACE,
                "--status-port",
                str(status),
                "--health-port",
                str(health),
                "--status-stale",
                "0.2",
                "--health-stale",
                "0.4",
                "--log",
                str(path),
            ],
            stdout=subprocess.DEVNULL,
            stderr=error,
        )
        try:
            wait_until(lambda: any(e["outcome"] == "ready" for e in events(path)))
            yield process
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
            process.wait(timeout=5)


class TrackingTests(unittest.TestCase):
    def outcomes(self, events):
        return [event["outcome"] for event in events]

    def test_reorder_window_and_gap_finalization(self):
        state = StreamState(0.5)
        self.assertEqual(
            self.outcomes(state.observe(100, 0)), ["baseline", "accepted_publication"]
        )
        self.assertIn("gap_pending", self.outcomes(state.observe(103, 0.1)))
        self.assertEqual(state.missing, {101, 102})
        self.assertEqual(
            self.outcomes(state.observe(101, 0.2)), ["reordered_publication"]
        )
        self.assertEqual(
            self.outcomes(state.observe(101, 0.3)), ["duplicate_publication"]
        )
        self.assertEqual(state.last_progress, 0.1)
        self.assertNotIn("gap_final", self.outcomes(state.observe(165, 0.4)))
        final = state.observe(166, 0.5)
        self.assertEqual(
            next(e["sequences"] for e in final if e["outcome"] == "gap_final"), [102]
        )
        self.assertLessEqual(len(state.missing), 63)
        self.assertLessEqual(len(state.seen), 64)
        self.assertEqual(self.outcomes(state.observe(100, 0.6)), ["late_publication"])
        self.assertTrue(state.finish("run_end"))
        self.assertFalse(state.missing)

    def test_wrap_discontinuity_and_freshness(self):
        state = StreamState(0.5)
        state.observe(MASK - 1, 0)
        self.assertIn("gap_pending", self.outcomes(state.observe(0, 0.1)))
        self.assertEqual(
            self.outcomes(state.observe(MASK, 0.2)), ["reordered_publication"]
        )
        self.assertEqual(self.outcomes(state.tick(0.61)), ["stale"])
        self.assertEqual(
            self.outcomes(state.observe(0, 0.7)), ["duplicate_publication"]
        )
        self.assertTrue(state.stale)
        self.assertEqual(self.outcomes(state.observe(HALF, 0.8)), ["discontinuity"])
        self.assertTrue(state.stale)
        self.assertEqual(
            self.outcomes(state.observe(1, 0.9)), ["recovered", "accepted_publication"]
        )
        result = state.observe(10000, 1.0)
        self.assertEqual(
            self.outcomes(result), ["discontinuity", "accepted_publication"]
        )
        self.assertFalse(state.missing)

    def test_boot_change_retires_both_streams(self):
        tracker = DeviceStreams()
        status = copy.deepcopy(CASES["valid"][11]["message"])
        tracker.observe(status, 0)
        status["sequence"] = 2
        tracker.observe(status, 0.1)
        old = copy.deepcopy(status)
        status["boot_id"] = "1111111111111111"
        status["sequence"] = 0
        changed = tracker.observe(status, 0.2)
        self.assertIn("session_change", self.outcomes(changed))
        self.assertIn("gap_final", self.outcomes(changed))
        self.assertEqual(self.outcomes(tracker.observe(old, 0.3)), ["retired_session"])
        health = copy.deepcopy(CASES["valid"][-2]["message"])
        self.assertEqual(
            self.outcomes(tracker.observe(health, 0.4)), ["retired_session"]
        )
        self.assertIsNone(tracker.streams[22].highest)
        self.assertEqual(tracker.retired.maxlen, 8)


@unittest.skipUnless(
    sys.platform.startswith("linux"), "Multicast process/fault harness requires Linux"
)
class MulticastTests(unittest.TestCase):
    def test_two_listeners_while_commands_continue(self):
        for format in FORMATS:
            status, health = ports()
            with self.subTest(format=format), tempfile.TemporaryDirectory() as temp:
                first, second = Path(temp) / "first.jsonl", Path(temp) / "second.jsonl"
                with listener_process(format, status, health, first), listener_process(
                    format, status, health, second
                ):
                    with server(format, extra=options(status, health)) as (
                        endpoint,
                        server_log,
                    ), socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                        sock.bind((INTERFACE, 0))
                        output = io.StringIO()
                        client = Client(
                            sock,
                            endpoint,
                            format,
                            "board-01",
                            1,
                            EventLog(output, format=format),
                        )
                        # Prove publication is independent of commands and listener acknowledgement.
                        wait_until(
                            lambda: all(
                                {
                                    e.get("kind")
                                    for e in events(path)
                                    if e["outcome"] == "accepted_publication"
                                }
                                == {21, 22}
                                for path in (first, second)
                            )
                        )
                        for _ in range(5):
                            for command in ("info", "status", "health"):
                                self.assertIsNotNone(client.request(command))
                        for path in (first, second):
                            accepted = [
                                e
                                for e in events(path)
                                if e["outcome"] == "accepted_publication"
                            ]
                            for event in accepted:
                                self.assertEqual(
                                    event["peer"][1],
                                    status if event["kind"] == 21 else health,
                                )
                                self.assertNotIn("request_id", event["message"])
                                self.assertNotIn("client_session", event["message"])
                                if sys.platform.startswith("linux"):
                                    self.assertEqual(event["observed_ttl"], 1)
                                    self.assertEqual(event["destination"], GROUP)
                        first_ids = {
                            (e["kind"], e["message"]["sequence"])
                            for e in events(first)
                            if e["outcome"] == "accepted_publication"
                        }
                        second_ids = {
                            (e["kind"], e["message"]["sequence"])
                            for e in events(second)
                            if e["outcome"] == "accepted_publication"
                        }
                        self.assertTrue(
                            {kind for kind, _ in first_ids & second_ids} == {21, 22}
                        )
                        self.assertTrue(
                            all(
                                e["outcome"] != "timeout"
                                for e in map(json.loads, output.getvalue().splitlines())
                            )
                        )
                    # Server stopped: independent stale alarms for both streams.
                    for path in (first, second):
                        wait_until(
                            lambda: {
                                e.get("kind")
                                for e in events(path)
                                if e["outcome"] == "stale"
                            }
                            == {21, 22}
                        )
                    # A reboot is a new session, not a giant sequence gap or a false recovery of old data.
                    with server(format, extra=options(status, health)):
                        for path in (first, second):
                            wait_until(
                                lambda: any(
                                    e["outcome"] == "session_change"
                                    and e.get("previous_boot_id") is not None
                                    for e in events(path)
                                )
                            )

    @unittest.skipUnless(
        sys.platform.startswith("linux"), "SIGSTOP/SIGCONT timing test is Linux-only"
    )
    def test_pause_skips_slots_and_resumes_without_burst(self):
        status, health = ports()
        with tempfile.TemporaryDirectory() as temp, listener_process(
            "json", status, health, Path(temp) / "listener.jsonl"
        ):
            path = Path(temp) / "server.jsonl"
            with open(Path(temp) / "stderr", "w") as error:
                process = subprocess.Popen(
                    [
                        BINARY,
                        "serve",
                        "--format",
                        "json",
                        "--bind",
                        f"{INTERFACE}:0",
                        "--log",
                        str(path),
                        *options(status, health),
                    ],
                    stderr=error,
                )
                try:
                    wait_until(
                        lambda: len(
                            [
                                e
                                for e in events(path)
                                if e["outcome"] == "sent_publication"
                            ]
                        )
                        >= 4
                    )
                    os.kill(process.pid, signal.SIGSTOP)
                    time.sleep(0.55)
                    os.kill(process.pid, signal.SIGCONT)
                    wait_until(
                        lambda: any(
                            e["outcome"] == "publication_after_skip"
                            for e in events(path)
                        )
                    )
                    wait_until(
                        lambda: {
                            e.get("kind")
                            for e in events(Path(temp) / "listener.jsonl")
                            if e["outcome"] == "recovered"
                        }
                        == {21, 22}
                    )
                    wait_until(
                        lambda: any(
                            e.get("message", {})
                            .get("health", {})
                            .get("skipped_publications", 0)
                            > 0
                            for e in events(path)
                            if e.get("message")
                        )
                    )
                    pubs = [
                        e
                        for e in events(path)
                        if e["outcome"]
                        in ("sent_publication", "publication_after_skip")
                    ]
                    for kind in (21, 22):
                        samples = [e for e in pubs if e["message"]["kind"] == kind]
                        paused = next(
                            i
                            for i, e in enumerate(samples)
                            if e["outcome"] == "publication_after_skip"
                        )
                        self.assertGreater(
                            samples[paused]["message"]["sequence"]
                            - samples[paused - 1]["message"]["sequence"],
                            1,
                        )
                finally:
                    os.kill(process.pid, signal.SIGCONT)
                    process.terminate()
                    process.wait(timeout=5)

    def test_slow_log_consumer_does_not_block_publications_or_commands(self):
        status, health = ports()
        fast = list(options(status, health))
        fast[fast.index("--status-hz") + 1] = "1000"
        with tempfile.TemporaryDirectory() as temp, open(
            Path(temp) / "stderr", "w"
        ) as error:
            process = subprocess.Popen(
                [
                    BINARY,
                    "serve",
                    "--format",
                    "json",
                    "--bind",
                    f"{INTERFACE}:0",
                    *fast,
                ],
                stdout=subprocess.PIPE,
                stderr=error,
                text=True,
            )
            try:
                ready = json.loads(process.stdout.readline())
                while ready["outcome"] != "ready":
                    ready = json.loads(process.stdout.readline())
                ip, port = ready["local_endpoint"].rsplit(":", 1)
                with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
                    sock.bind((INTERFACE, 0))
                    # Deliberately leave the stdout pipe undrained until its queue fills.
                    time.sleep(0.6)
                    client = Client(
                        sock,
                        (ip, int(port)),
                        "json",
                        "board-01",
                        1,
                        EventLog(io.StringIO(), format="json"),
                    )
                    self.assertIsNotNone(client.request("health"))
                dropped = threading.Event()

                def drain():
                    for line in process.stdout:
                        if not line.endswith("\n"):
                            break  # Terminating a process can interrupt its final JSONL write.
                        if json.loads(line).get("log_dropped", 0) > 0:
                            dropped.set()

                reader = threading.Thread(target=drain)
                reader.start()
                self.assertTrue(
                    dropped.wait(3), "bounded logging did not report dropped records"
                )
            finally:
                process.terminate()
                process.wait(timeout=5)
                if "reader" in locals():
                    reader.join(timeout=2)
                process.stdout.close()

    def test_listener_rejects_invalid_without_sending_ack(self):
        status, health = ports()
        with tempfile.TemporaryDirectory() as temp, listener_process(
            "json", status, health, Path(temp) / "listener.jsonl"
        ):
            path = Path(temp) / "listener.jsonl"
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
                sender.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                sender.bind((INTERFACE, status))
                sender.setsockopt(
                    socket.IPPROTO_IP,
                    socket.IP_MULTICAST_IF,
                    socket.inet_aton(INTERFACE),
                )
                sender.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
                for wire in (
                    b"x" * 1025,
                    b"broken",
                    encode("json", CASES["valid"][0]["message"]),
                ):
                    sender.sendto(wire, (GROUP, status))
                wait_until(
                    lambda: {"size_error", "decode_error", "unexpected_publication"}
                    <= {e["outcome"] for e in events(path)}
                )
                sender.settimeout(0.1)
                with self.assertRaises(socket.timeout):
                    sender.recvfrom(65535)


if __name__ == "__main__":
    unittest.main(verbosity=2)
