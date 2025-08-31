#!/usr/bin/env python3
# /// script
# dependencies = []
# ///
"""
Benchmark Repository Setup Script

Downloads and configures test repositories for benchmarking.
"""

import subprocess
import sys
from pathlib import Path


def setup_fastapi():
    """Setup FastAPI test suite."""
    print("Setting up FastAPI...")
    
    repo_dir = Path("benchmark_workspace/fastapi")
    if repo_dir.exists():
        print("FastAPI already exists, skipping clone.")
        return repo_dir
        
    subprocess.run([
        'git', 'clone', '--depth', '1',
        'https://github.com/tiangolo/fastapi.git',
        str(repo_dir)
    ], check=True)
    
    # Install dependencies
    subprocess.run([
        'uv', 'pip', 'install', '-e', '.'
    ], cwd=repo_dir, check=True)
    
    print(f"FastAPI setup complete: {repo_dir}")
    return repo_dir


def setup_demo_suite():
    """Setup the existing phase3_demo suite for quick testing."""
    print("Setting up demo test suite...")
    
    demo_dir = Path("examples/phase3_demo")
    if not demo_dir.exists():
        print("Demo suite not found, creating minimal test suite...")
        demo_dir.mkdir(parents=True, exist_ok=True)
        
        # Create a simple test file
        test_content = '''"""Demo test suite for benchmarking."""

import pytest
import time


def test_fast():
    """Fast test that completes quickly."""
    assert 1 + 1 == 2


def test_medium():
    """Medium test with some computation."""
    result = sum(range(1000))
    assert result == 499500


def test_slow():
    """Slower test for timing variation."""
    time.sleep(0.01)
    assert True


@pytest.mark.parametrize("value", range(10))
def test_parametrized(value):
    """Parametrized test to create many test cases."""
    assert value >= 0


class TestCalculator:
    """Test class with multiple methods."""
    
    def test_add(self):
        assert 2 + 2 == 4
        
    def test_subtract(self):
        assert 5 - 3 == 2
        
    def test_multiply(self):
        assert 3 * 4 == 12
        
    def test_divide(self):
        assert 10 / 2 == 5
'''
        
        (demo_dir / "test_demo.py").write_text(test_content)
        
        # Create conftest.py
        conftest_content = '''"""Demo conftest for benchmarking."""

import pytest


@pytest.fixture
def sample_data():
    """Sample test fixture."""
    return {"key": "value", "numbers": [1, 2, 3, 4, 5]}


@pytest.fixture(scope="session")
def expensive_setup():
    """Expensive setup fixture."""
    # Simulate expensive setup
    return "expensive_resource"
'''
        
        (demo_dir / "conftest.py").write_text(conftest_content)
        
        # Create simple module to import
        module_content = '''"""Simple module for import testing."""

def add(a, b):
    """Add two numbers."""
    return a + b


def multiply(a, b):
    """Multiply two numbers.""" 
    return a * b


CONSTANT = 42
'''
        
        (demo_dir / "calculator.py").write_text(module_content)
        
    print(f"Demo suite ready: {demo_dir}")
    return demo_dir


def main():
    """Setup benchmark repositories."""
    print("Setting up benchmark repositories...")
    
    # Create workspace directory
    workspace = Path("benchmark_workspace")
    workspace.mkdir(exist_ok=True)
    
    # Setup repositories
    try:
        # Start with demo suite for quick testing
        setup_demo_suite()
        
        # Only setup external repos if requested
        if "--external" in sys.argv:
            setup_fastapi()
            
    except subprocess.CalledProcessError as e:
        print(f"Setup failed: {e}")
        sys.exit(1)
        
    print("Benchmark repository setup complete!")


if __name__ == '__main__':
    main()