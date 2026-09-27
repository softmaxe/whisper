"""Shared fixtures: the real timeline, exported fresh from the TypeScript source once per test run."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from whisper_audio.timeline import load_timeline

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def timeline_json(tmp_path_factory) -> Path:
    out = tmp_path_factory.mktemp("timeline") / "timeline.json"
    subprocess.run(["node", "scripts/export-timeline.ts", str(out)], cwd=ROOT, check=True, capture_output=True)
    return out


@pytest.fixture(scope="session")
def timeline(timeline_json):
    return load_timeline(timeline_json)
