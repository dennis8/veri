"""
Phase 14: Multi-worker smoke tests
Runs veri with multiple workers on a trivial test suite.
"""

import os
import subprocess
import tempfile
from pathlib import Path


def test_multiworker_smoke():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "test_ok.py").write_text(
            """
def test_ok():
    assert 1+1 == 2
"""
        )

        env = os.environ.copy()
        # Disable allowlist for the smoke; pool is always enabled
        # Prepend repo .bin to PATH so `veri` resolves to dev binary
        env["PATH"] = (
            str(Path(__file__).resolve().parents[1] / ".bin")
            + os.pathsep
            + env.get("PATH", "")
        )

        result = subprocess.run(
            ["veri", "--disable-allowlist", "--workers", "2", "-v"],
            cwd=d,
            capture_output=True,
            text=True,
            env=env,
        )
        assert result.returncode in (0, 1), result.stdout + "\n" + result.stderr
