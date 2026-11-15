"""Exercise the real AppKit/AX process boundary using a disposable draft.

Run on macOS with Accessibility permission already granted:
    uv run --no-project tests/native/check_text_observation.py

The fixture creates its own window and never reads or modifies user documents.
This smoke check does not establish Chromium compatibility or input-method behavior.
"""

import json
import queue
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from typing import Any


def lines(process: subprocess.Popen[str]) -> queue.Queue[str]:
    output: queue.Queue[str] = queue.Queue()

    def read() -> None:
        assert process.stdout is not None
        for line in process.stdout:
            output.put(line.rstrip())

    threading.Thread(target=read, daemon=True).start()
    return output


def write(process: subprocess.Popen[str], value: str) -> None:
    assert process.stdin is not None
    process.stdin.write(value + "\n")
    process.stdin.flush()


def event(output: queue.Queue[str], kind: str, text: str | None = None) -> dict[str, Any]:
    deadline = time.monotonic() + 4
    while time.monotonic() < deadline:
        value = json.loads(output.get(timeout=4))
        summary = dict(value)
        if len(summary.get("value", "")) > 160:
            summary["value_length"] = len(summary["value"])
            summary["value_tail"] = summary.pop("value")[-80:]
        print(json.dumps(summary, ensure_ascii=False), flush=True)
        if value["kind"] == kind and (text is None or value.get("value") == text):
            return value
    raise AssertionError(kind)


def main() -> None:
    root = Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory(prefix="whisper-ax-smoke-") as directory:
        temporary = Path(directory)
        fixture_binary = temporary / "fixture"
        helper_binary = temporary / "observer"
        subprocess.run(["swiftc", str(root / "resources/native/macos-text-monitor.swift"), "-o", str(helper_binary)], check=True)
        subprocess.run(["swiftc", str(Path(__file__).with_name("text_observation_fixture.swift")), "-o", str(fixture_binary)], check=True)
        check(fixture_binary, helper_binary)


def check(fixture_binary: Path, helper_binary: Path) -> None:
    fixture = subprocess.Popen([str(fixture_binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    helper: subprocess.Popen[str] | None = None
    try:
        fixture_output = lines(fixture)
        pid = int(fixture_output.get(timeout=4))
        time.sleep(0.2)
        helper = subprocess.Popen([str(helper_binary), "--observe", str(pid)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        output = lines(helper)
        write(helper, json.dumps({"command": "prepare", "ticket": 1}))
        prepared = event(output, "prepared")
        assert prepared.get("value") == "", prepared
        assert prepared["start"] == 0
        write(fixture, "paste")
        assert fixture_output.get(timeout=4) == "DONE:paste"
        write(helper, json.dumps({"command": "commit", "ticket": 1}))
        pasted = event(output, "value", "hello world")
        write(fixture, "correct")
        assert fixture_output.get(timeout=4) == "DONE:correct"
        corrected = event(output, "value", "hello World")
        assert corrected["field"] == prepared["field"] == pasted["field"]
        write(fixture, "clear")
        assert fixture_output.get(timeout=4) == "DONE:clear"
        empty = event(output, "value", "")
        assert empty["kind"] != "sent", "deletion without send must remain a replacement"
        write(helper, json.dumps({"command":"snapshot", "ticket":2}))
        event(output, "synced")
        write(fixture, "paste")
        assert fixture_output.get(timeout=4) == "DONE:paste"
        event(output, "value", "hello world")
        write(fixture, "correct")
        assert fixture_output.get(timeout=4) == "DONE:correct"
        event(output, "value", "hello World")
        write(fixture, "send")
        assert fixture_output.get(timeout=4) == "DONE:send"
        sent = event(output, "sent")
        assert sent["field"] == prepared["field"]
        write(fixture, "long")
        assert fixture_output.get(timeout=4) == "DONE:long"
        write(helper, json.dumps({"command": "prepare", "ticket": 3}))
        long = event(output, "prepared")
        assert long["value"] == "existing " * 2000 + "克劳德"
        assert long["start"] == len(long["value"])
        write(helper, json.dumps({"command": "commit", "ticket": 3}))
        event(output, "value", long["value"])
        write(fixture, "long-correct")
        assert fixture_output.get(timeout=4) == "DONE:long-correct"
        event(output, "value", "existing " * 2000 + "Claude")
        print("PASS: real AX preparation, delivery, immediate edit, temporary clear, snapshot barrier, accessible Send button, long editor tail")
    finally:
        if helper is not None and helper.poll() is None:
            helper.terminate()
            helper.wait(timeout=3)
        if fixture.poll() is None:
            write(fixture, "quit")
            try:
                fixture.wait(timeout=3)
            except subprocess.TimeoutExpired:
                fixture.kill()
                fixture.wait(timeout=3)


if __name__ == "__main__":
    main()
