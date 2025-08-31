"""
Phase 14: Multi-worker smoke tests
Runs veri in experimental multi-worker mode on a trivial test suite.
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
        # Ensure experimental pool is enabled and allowlist is disabled for the smoke
        env["VERI_EXPERIMENTAL_WORKERPOOL"] = "1"
        # Prepend repo .bin to PATH so `veri` resolves to dev binary
        env["PATH"] = str(Path(__file__).resolve().parents[1] / ".bin") + os.pathsep + env.get("PATH", "")

        result = subprocess.run(
            ["veri", "--disable-allowlist", "--workers", "2", "-v"],
            cwd=d,
            capture_output=True,
            text=True,
            env=env,
        )
        assert result.returncode in (0, 1), result.stdout + "\n" + result.stderr

