"""
Integration tests for multi-worker execution, coverage reports, and timings.
These tests invoke the dev `veri` binary (resolved via examples/conftest.py).
"""

import os
import json
import subprocess
import tempfile
from pathlib import Path


def _run(cmd, cwd: Path, env=None):
    e = os.environ.copy()
    if env:
        e.update(env)
    # Keep runs deterministic for CI and local
    e.setdefault("VERI_DISABLE_ALLOWLIST", "1")
    repo_root = Path(__file__).resolve().parents[1]
    bin_dir = repo_root / ".bin"
    if bin_dir.is_dir():
        e["PATH"] = str(bin_dir) + os.pathsep + e.get("PATH", "")
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=e)
    return p


def test_parallel_scheduling_and_output():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "test_a.py").write_text(
            """
def test_one():
    assert True

def test_two():
    assert True
"""
        )

        p = _run(["veri", "--workers", "2", "-a", "-v"], d)
        assert p.returncode in (0, 1)
        cache = d / ".veri" / "cache"
        assert (cache / "tests.index.json").exists()


def test_timings_written_and_loaded():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "test_b.py").write_text(
            """
import time

def test_slow():
    time.sleep(0.01)
    assert True
"""
        )

        # First run writes timings
        p1 = _run(["veri", "--workers", "2", "-a"], d)
        assert p1.returncode in (0, 1)
        timings = d / ".veri" / "cache" / "timings.json"
        assert timings.exists(), f"timings.json missing: {timings}"

        # Second run should load historical timings (verbose)
        p2 = _run(["veri", "--workers", "2", "-a", "-v"], d)
        out = p2.stdout + p2.stderr
        assert "Loaded historical timing data" in out


def test_coverage_reports_generated():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "test_cov.py").write_text(
            """
def test_ok():
    assert 1+1 == 2
"""
        )

        p = _run(["veri", "--workers", "2", "--cov-merge-full", "-a", "-v"], d)
        assert p.returncode in (0, 1)
        reports = d / "reports"
        # XML and JSON from our collector
        assert (reports / "coverage.xml").exists()
        assert (reports / "coverage.json").exists()
        # HTML from coverage.py (best-effort)
        assert (reports / "htmlcov").exists()


def test_summary_rollup_includes_skipped():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "test_skip.py").write_text(
            """
import pytest

@pytest.mark.skip(reason="demo")
def test_skip_me():
    pass

def test_ok():
    assert True
"""
        )
        p = _run(["veri", "--workers", "1", "-a"], d)
        out = p.stdout + p.stderr
        assert "Summary:" in out
        assert "skipped" in out


def test_exit_codes_mapping_for_failures():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "test_fail.py").write_text(
            """
def test_fail():
    assert False
"""
        )
        p = _run(["veri", "--workers", "2", "-a"], d)
        assert p.returncode == 1
