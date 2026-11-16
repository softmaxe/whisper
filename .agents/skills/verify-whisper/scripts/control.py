#!/usr/bin/env python3
"""Own an isolated Whisper process and retain verification evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import plistlib
import shutil
import signal
import sqlite3
import subprocess
import time
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[4]


def command(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def identity(pid: int) -> str:
    result = subprocess.run(
        ["/bin/ps", "-p", str(pid), "-o", "lstart=,pgid=,comm="],
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout.strip()


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def load(run: Path) -> dict[str, Any]:
    data: dict[str, Any] = json.loads((run / "run.json").read_text())
    if Path(data["run"]) != run or Path(data["root"]) != ROOT:
        raise RuntimeError("Run belongs to a different directory or checkout")
    return data


def database_state(run: Path) -> dict[str, Any]:
    database = run / "scratch" / "data" / "transcriptions.db"
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("PRAGMA query_only = ON")
        return {
            "schema_version": connection.execute("PRAGMA user_version").fetchone()[0],
            "dictionary": connection.execute(
                "SELECT word, source, use_count FROM custom_dictionary ORDER BY id"
            ).fetchall(),
            "snippets": connection.execute(
                "SELECT trigger, replacement FROM snippets ORDER BY id"
            ).fetchall(),
            "transcription_count": connection.execute(
                "SELECT count(*) FROM transcriptions"
            ).fetchone()[0],
            "analytics_count": connection.execute(
                "SELECT count(*) FROM analytics_events"
            ).fetchone()[0],
        }


def doctor(run: Path) -> dict[str, Any]:
    data = load(run)
    pid = data["pid"]
    if not data["identity"] or identity(pid) != data["identity"]:
        raise RuntimeError("Owned Whisper process is absent or its identity changed")
    if int(command("/bin/ps", "-p", str(pid), "-o", "pgid=")) != pid:
        raise RuntimeError("Whisper no longer owns its process group")
    binary = Path(data["binary"])
    if (
        digest(binary) != data["binary_sha256"]
        or digest(Path(data["source_binary"])) != data["binary_sha256"]
    ):
        raise RuntimeError("Binary changed since launch; clean up and launch again")
    files = command("/usr/sbin/lsof", "-a", "-p", str(pid), "-d", "txt", "-Fn")
    if f"n{binary}\n" not in files + "\n":
        raise RuntimeError("Process is not running the recorded checkout binary")
    lock = run / "scratch" / "data" / "instance.lock"
    if lock.read_text().strip() != str(pid):
        raise RuntimeError("Data directory lock does not name the owned process")
    if "retention cleanup:" not in (run / "app.log").read_text():
        raise RuntimeError("Startup retention cleanup has not completed")
    state = database_state(run)
    if state["schema_version"] != 4:
        raise RuntimeError("Expected database schema version 4")
    return {
        "status": "ready-for-ui-check",
        "pid": pid,
        "binary": str(binary),
        "bundle": data["bundle"],
        "cua_bind": f"var app = await cua.getApp({json.dumps(data['bundle'])});",
        "cua_evidence": f"var evidence = {json.dumps(str(run))};",
        "revision": data["revision"],
        "scene": "home",
        **state,
    }


def group_alive(pid: int) -> bool:
    # macOS can report EPERM for killpg(..., 0) on an exiting group. Inspect
    # live members instead; a child zombie is reaped by launch's Popen handle.
    rows = command("/bin/ps", "-axo", "pgid=,stat=").splitlines()
    return any(
        int(fields[0]) == pid and not fields[1].startswith("Z")
        for row in rows
        if len(fields := row.split()) == 2
    )


def terminate_group(pid: int) -> None:
    if not group_alive(pid):
        return
    try:
        os.killpg(pid, signal.SIGTERM)
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if not group_alive(pid):
                break
            time.sleep(0.1)
        else:
            os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def cleanup(run: Path) -> dict[str, Any]:
    data = load(run)
    if data.get("cleaned"):
        return {"status": "already-cleaned", "evidence": str(run)}
    pid = data.get("pid")
    if pid is not None:
        current = identity(pid)
        if current and current != data["identity"]:
            raise RuntimeError(
                "PID was reused; refusing to signal it or remove scratch state"
            )
        # With a dead leader, still reclaim its recorded process group and helpers.
        if current and int(command("/bin/ps", "-p", str(pid), "-o", "pgid=")) != pid:
            raise RuntimeError("Process group changed; refusing cleanup")
        terminate_group(pid)
    if (run / "scratch").exists():
        shutil.rmtree(run / "scratch")
    data["cleaned"] = True
    write_json(run / "run.json", data)
    report = {
        "status": "cleaned",
        "evidence": str(run),
        "retained_files": sorted(p.name for p in run.iterdir() if p.is_file()),
    }
    write_json(run / "cleanup.json", report)
    return report


def launch(run: Path) -> dict[str, Any]:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise RuntimeError(
            "Whisper verification requires an Apple Silicon macOS GUI session"
        )
    source_binary = ROOT / "target" / "debug" / "whisper"
    if not source_binary.is_file():
        raise RuntimeError(
            "Build first with cargo build --locked (default target directory)"
        )
    run.mkdir(parents=True, exist_ok=False)
    scratch = run / "scratch"
    process: subprocess.Popen[Any] | None = None
    metadata: dict[str, Any] = {"run": str(run), "root": str(ROOT), "pid": None}
    try:
        write_json(run / "run.json", metadata)
        data_dir = scratch / "data"
        data_dir.mkdir(parents=True)
        # CUA cannot bind a bare Mach-O. Retain the repository's identifiers
        # in a disposable wrapper with the unchanged debug executable.
        bundle = scratch / "Whisper.app"
        binary = bundle / "Contents" / "MacOS" / "whisper"
        binary.parent.mkdir(parents=True)
        shutil.copy2(source_binary, binary)
        with (ROOT / "packaging" / "Info.plist").open("rb") as stream:
            info = plistlib.load(stream)
        with (ROOT / "Cargo.toml").open("rb") as stream:
            version = tomllib.load(stream)["package"]["version"]
        info["CFBundleShortVersionString"] = version
        info["CFBundleVersion"] = version
        with (bundle / "Contents" / "Info.plist").open("wb") as stream:
            plistlib.dump(info, stream)
        env = os.environ.copy()
        # home selects the initial view and disables global hotkey listeners;
        # it injects no rows or simulated Dictionary actions.
        for key in list(env):
            if key.startswith("WHISPER_"):
                del env[key]
        env.update(
            WHISPER_USER_DATA_DIR=str(data_dir),
            WHISPER_DEV_SCENE="home",
            WHISPER_HELPERS_DIR=str(ROOT / "resources" / "bin"),
            RUST_LOG="info",
        )
        metadata.update(
            binary=str(binary),
            source_binary=str(source_binary),
            bundle=str(bundle),
            binary_sha256=digest(binary),
            revision=command("git", "-C", str(ROOT), "rev-parse", "HEAD"),
            initial_data_entries=[],
            scene="home",
        )
        with (run / "app.log").open("w") as log:
            process = subprocess.Popen(
                [str(binary)],
                cwd=ROOT,
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
        metadata.update(pid=process.pid, identity=identity(process.pid))
        write_json(run / "run.json", metadata)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(
                    f"Whisper exited {process.returncode}; inspect app.log"
                )
            try:
                return doctor(run)
            except (RuntimeError, sqlite3.Error, FileNotFoundError):
                time.sleep(0.2)
        raise RuntimeError(
            "Startup did not become ready within 30 seconds; inspect app.log"
        )
    except BaseException:
        # Keep the live Popen handle authoritative if metadata creation failed.
        # Terminate before any further filesystem writes, including on disk errors.
        if process is not None:
            terminate_group(process.pid)
            process.wait(timeout=5)
        if scratch.exists():
            shutil.rmtree(scratch)
        metadata["cleaned"] = True
        write_json(run / "run.json", metadata)
        write_json(run / "cleanup.json", {"status": "cleaned-after-launch-failure"})
        raise


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["launch", "doctor", "state", "cleanup"])
    parser.add_argument("--run-dir", type=Path, required=True)
    args = parser.parse_args()
    run = args.run_dir.resolve()
    if not run.is_relative_to(ROOT / ".e2e" / "verify-whisper"):
        parser.error("--run-dir must be inside this checkout's .e2e/verify-whisper")
    actions = {
        "launch": launch,
        "doctor": doctor,
        "state": database_state,
        "cleanup": cleanup,
    }
    print(json.dumps(actions[args.action](run), indent=2))


if __name__ == "__main__":
    main()
