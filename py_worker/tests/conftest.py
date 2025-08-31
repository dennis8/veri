"""Test configuration and fixtures for veri_worker tests."""

import tempfile
from pathlib import Path

import pytest


@pytest.fixture
def temp_work_dir():
    """Create a temporary working directory for tests."""
    with tempfile.TemporaryDirectory() as temp_dir:
        yield Path(temp_dir)


@pytest.fixture
def temp_cache_dir(temp_work_dir):
    """Create a temporary cache directory."""
    cache_dir = temp_work_dir / ".veri" / "cache"
    cache_dir.mkdir(parents=True, exist_ok=True)
    return cache_dir


@pytest.fixture
def sample_module_map():
    """Sample module map for testing."""
    return {
        "version": "0.1.0",
        "generated_at": "2025-08-29T12:00:00Z",
        "modules": {
            "src/calculator.py": {
                "module_name": "src.calculator",
                "path": "src/calculator.py",
            },
            "tests/test_calculator.py": {
                "module_name": "tests.test_calculator",
                "path": "tests/test_calculator.py",
            },
        },
    }


@pytest.fixture
def sample_python_file(temp_work_dir):
    """Create a sample Python file with imports."""
    src_dir = temp_work_dir / "src"
    src_dir.mkdir(exist_ok=True)

    calc_file = src_dir / "calculator.py"
    calc_file.write_text("""
import math
from typing import Union
import os

def add(a: Union[int, float], b: Union[int, float]) -> Union[int, float]:
    return a + b

def divide(a: Union[int, float], b: Union[int, float]) -> float:
    if b == 0:
        raise ValueError("Cannot divide by zero")
    return a / b
""")

    return calc_file
