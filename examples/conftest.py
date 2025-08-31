"""Ensure example tests can locate the dev `veri` binary.

This prepends the repo `.bin` directory (which symlinks `veri` to the
debug build) to PATH so subprocess calls to `veri` resolve during tests.
"""

import os
from pathlib import Path


def pytest_sessionstart(session):
    repo_root = Path(__file__).resolve().parents[1]
    bin_dir = repo_root / ".bin"
    if bin_dir.is_dir():
        os.environ["PATH"] = str(bin_dir) + os.pathsep + os.environ.get("PATH", "")
