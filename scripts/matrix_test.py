#!/usr/bin/env python3
"""
Matrix testing script for veri compatibility verification.

This script runs veri across different Python versions, operating systems,
and environment types to verify compatibility.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple
import platform
import venv


class MatrixTestRunner:
    """Runs veri compatibility tests across different environments."""

    def __init__(self, veri_binary: str = "veri"):
        self.veri_binary = veri_binary
        self.results = {}

    def detect_environment(self) -> Dict[str, str]:
        """Detect current environment characteristics."""
        return {
            "python_version": f"{sys.version_info.major}.{sys.version_info.minor}",
            "python_full_version": sys.version.split()[0],
            "os": platform.system().lower(),
            "arch": platform.machine(),
            "platform": platform.platform(),
        }

    def create_test_project(self, test_dir: Path) -> None:
        """Create a minimal test project for compatibility testing."""
        # Create basic project structure
        (test_dir / "src").mkdir(exist_ok=True)
        (test_dir / "tests").mkdir(exist_ok=True)

        # Create pyproject.toml
        pyproject_content = """
[project]
name = "veri-test-project"
version = "0.1.0"
description = "Test project for veri compatibility"
requires-python = ">=3.8"
dependencies = []

[build-system]
requires = ["setuptools>=61.0"]
build-backend = "setuptools.build_meta"

[tool.pytest.ini_options]
testpaths = ["tests"]
python_files = ["test_*.py"]
python_classes = ["Test*"]
python_functions = ["test_*"]

[tool.veri]
log_level = "INFO"
workers = "auto"
"""
        (test_dir / "pyproject.toml").write_text(pyproject_content.strip())

        # Create source code
        src_content = '''
def add(a, b):
    """Add two numbers."""
    return a + b

def multiply(a, b):
    """Multiply two numbers."""
    return a * b

class Calculator:
    """Simple calculator class."""
    
    def add(self, a, b):
        return add(a, b)
    
    def multiply(self, a, b):
        return multiply(a, b)
    
    def divide(self, a, b):
        if b == 0:
            raise ValueError("Cannot divide by zero")
        return a / b
'''
        (test_dir / "src" / "calculator.py").write_text(src_content.strip())

        # Create tests
        test_content = '''
import pytest
import sys
from pathlib import Path

# Add src to path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from calculator import add, multiply, Calculator


def test_add():
    """Test addition function."""
    assert add(2, 3) == 5
    assert add(-1, 1) == 0
    assert add(0, 0) == 0


def test_multiply():
    """Test multiplication function."""
    assert multiply(2, 3) == 6
    assert multiply(-1, 5) == -5
    assert multiply(0, 10) == 0


class TestCalculator:
    """Test calculator class."""
    
    def setup_method(self):
        self.calc = Calculator()
    
    def test_calculator_add(self):
        assert self.calc.add(1, 2) == 3
    
    def test_calculator_multiply(self):
        assert self.calc.multiply(3, 4) == 12
    
    def test_calculator_divide(self):
        assert self.calc.divide(10, 2) == 5.0
        
        with pytest.raises(ValueError, match="Cannot divide by zero"):
            self.calc.divide(1, 0)


@pytest.mark.parametrize("a,b,expected", [
    (1, 2, 3),
    (0, 0, 0),
    (-1, 1, 0),
    (100, 200, 300),
])
def test_add_parametrized(a, b, expected):
    """Test addition with parameters."""
    assert add(a, b) == expected


def test_slow_operation():
    """Test that might be timing sensitive."""
    import time
    start = time.time()
    result = add(1, 1)
    end = time.time()
    
    assert result == 2
    assert end - start < 1.0  # Should be very fast


@pytest.mark.asyncio
async def test_async_operation():
    """Test async functionality if pytest-asyncio is available."""
    import asyncio
    
    async def async_add(a, b):
        await asyncio.sleep(0.01)  # Simulate async work
        return a + b
    
    result = await async_add(2, 3)
    assert result == 5
'''
        (test_dir / "tests" / "test_basic.py").write_text(test_content.strip())

        # Create conftest.py with fixtures
        conftest_content = '''
import pytest


@pytest.fixture
def sample_data():
    """Provide sample data for tests."""
    return {
        "numbers": [1, 2, 3, 4, 5],
        "strings": ["hello", "world"],
        "config": {"debug": True}
    }


@pytest.fixture(scope="session")
def session_data():
    """Session-scoped fixture."""
    return {"session_id": "test_session_123"}


def pytest_configure(config):
    """Configure pytest."""
    config.addinivalue_line(
        "markers", "slow: marks tests as slow"
    )
    config.addinivalue_line(
        "markers", "integration: marks tests as integration tests"
    )
'''
        (test_dir / "tests" / "conftest.py").write_text(conftest_content.strip())

        # Create a test with potential plugin conflicts
        plugin_test_content = '''
import pytest


def test_mock_functionality():
    """Test that would use pytest-mock if available."""
    try:
        from unittest.mock import Mock
        mock_obj = Mock()
        mock_obj.method.return_value = "mocked"
        assert mock_obj.method() == "mocked"
    except ImportError:
        pytest.skip("Mock not available")


@pytest.mark.slow
def test_marked_slow():
    """Test marked as slow."""
    import time
    time.sleep(0.1)
    assert True


def test_with_fixture(sample_data):
    """Test using fixtures."""
    assert "numbers" in sample_data
    assert len(sample_data["numbers"]) == 5
'''
        (test_dir / "tests" / "test_plugins.py").write_text(plugin_test_content.strip())

    def run_compatibility_test(self, test_dir: Path, test_name: str) -> Dict:
        """Run a single compatibility test."""
        print(f"  Running {test_name}...")

        start_time = time.time()
        result = {
            "test_name": test_name,
            "environment": self.detect_environment(),
            "start_time": start_time,
            "success": False,
            "exit_code": None,
            "stdout": "",
            "stderr": "",
            "duration": 0.0,
            "error": None,
        }

        try:
            # Change to test directory
            original_cwd = os.getcwd()
            os.chdir(test_dir)

            # Build command based on test type
            if test_name == "basic_run":
                cmd = [self.veri_binary, "-v"]
            elif test_name == "all_tests":
                cmd = [self.veri_binary, "--all", "-v"]
            elif test_name == "parallel_execution":
                cmd = [self.veri_binary, "--workers", "2", "-v"]
            elif test_name == "coverage":
                cmd = [self.veri_binary, "--cov", "-v"]
            elif test_name == "explain_mode":
                cmd = [self.veri_binary, "--explain"]
            elif test_name == "pytest_engine":
                cmd = [self.veri_binary, "--engine", "pytest", "-v"]
            elif test_name == "marker_filter":
                cmd = [self.veri_binary, "-m", "slow", "-v"]
            elif test_name == "keyword_filter":
                cmd = [self.veri_binary, "-k", "add", "-v"]
            elif test_name == "junit_output":
                cmd = [self.veri_binary, "--junit-xml", "reports/junit.xml", "-v"]
            elif test_name == "help":
                cmd = [self.veri_binary, "--help"]
            elif test_name == "version":
                cmd = [self.veri_binary, "--version"]
            else:
                raise ValueError(f"Unknown test: {test_name}")

            # Run the command
            process = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=60,  # 60 second timeout
            )

            result.update(
                {
                    "success": process.returncode == 0
                    or test_name in ["help", "version", "explain_mode"],
                    "exit_code": process.returncode,
                    "stdout": process.stdout,
                    "stderr": process.stderr,
                    "duration": time.time() - start_time,
                }
            )

        except subprocess.TimeoutExpired:
            result["error"] = "Test timed out after 60 seconds"
            result["duration"] = time.time() - start_time
        except Exception as e:
            result["error"] = str(e)
            result["duration"] = time.time() - start_time
        finally:
            os.chdir(original_cwd)

        return result

    def run_all_tests(self, test_dir: Path) -> Dict:
        """Run all compatibility tests."""
        tests = [
            "version",
            "help",
            "explain_mode",
            "basic_run",
            "all_tests",
            "parallel_execution",
            "coverage",
            "pytest_engine",
            "marker_filter",
            "keyword_filter",
            "junit_output",
        ]

        results = {}
        print(f"Running {len(tests)} compatibility tests...")

        for test in tests:
            results[test] = self.run_compatibility_test(test_dir, test)

            # Print immediate result
            if results[test]["success"]:
                print(f"    ✅ {test}")
            else:
                print(f"    ❌ {test} - {results[test].get('error', 'Failed')}")

        return results

    def generate_report(self, results: Dict) -> Dict:
        """Generate a compatibility report."""
        total_tests = len(results)
        passed_tests = sum(1 for r in results.values() if r["success"])
        failed_tests = total_tests - passed_tests

        environment = list(results.values())[0]["environment"] if results else {}

        report = {
            "summary": {
                "total_tests": total_tests,
                "passed": passed_tests,
                "failed": failed_tests,
                "success_rate": (passed_tests / total_tests * 100)
                if total_tests > 0
                else 0,
                "environment": environment,
            },
            "results": results,
            "recommendations": self.generate_recommendations(results),
        }

        return report

    def generate_recommendations(self, results: Dict) -> List[str]:
        """Generate recommendations based on test results."""
        recommendations = []

        failed_tests = [
            name for name, result in results.items() if not result["success"]
        ]

        if not failed_tests:
            recommendations.append("✅ All compatibility tests passed!")
            return recommendations

        if "version" in failed_tests:
            recommendations.append(
                "⚠️  veri binary may not be properly installed or accessible"
            )

        if "basic_run" in failed_tests:
            recommendations.append(
                "⚠️  Basic test execution failed - check veri installation and Python environment"
            )

        if "parallel_execution" in failed_tests:
            recommendations.append(
                "⚠️  Parallel execution failed - may be a platform-specific issue"
            )

        if "coverage" in failed_tests:
            recommendations.append(
                "⚠️  Coverage collection failed - check if coverage dependencies are installed"
            )

        if "pytest_engine" in failed_tests:
            recommendations.append(
                "⚠️  Pytest engine fallback failed - check pytest installation"
            )

        # Check for plugin-related issues
        stderr_content = " ".join(r.get("stderr", "") for r in results.values())
        if (
            "plugin" in stderr_content.lower()
            or "incompatible" in stderr_content.lower()
        ):
            recommendations.append(
                "🔌 Plugin compatibility issues detected - consider updating plugins or using --engine pytest"
            )

        if len(failed_tests) > len(results) // 2:
            recommendations.append(
                "🚨 More than half of tests failed - this environment may have significant compatibility issues"
            )

        recommendations.append(
            "📖 For troubleshooting help: https://docs.veri.dev/troubleshooting"
        )

        return recommendations

    def print_report(self, report: Dict) -> None:
        """Print the compatibility report."""
        summary = report["summary"]

        print("\n" + "=" * 60)
        print("🔍 VERI COMPATIBILITY REPORT")
        print("=" * 60)

        # Environment info
        env = summary["environment"]
        print(f"\nEnvironment:")
        print(f"  Python: {env.get('python_full_version', 'unknown')}")
        print(f"  OS: {env.get('platform', 'unknown')}")
        print(f"  Architecture: {env.get('arch', 'unknown')}")

        # Summary
        print(f"\nTest Results:")
        print(f"  Total tests: {summary['total_tests']}")
        print(f"  Passed: {summary['passed']} ✅")
        print(f"  Failed: {summary['failed']} ❌")
        print(f"  Success rate: {summary['success_rate']:.1f}%")

        # Failed tests detail
        if summary["failed"] > 0:
            print(f"\nFailed Tests:")
            for name, result in report["results"].items():
                if not result["success"]:
                    print(f"  ❌ {name}")
                    if result.get("error"):
                        print(f"     Error: {result['error']}")
                    elif result.get("stderr"):
                        # Show first line of stderr
                        stderr_line = result["stderr"].split("\n")[0]
                        if stderr_line.strip():
                            print(f"     Stderr: {stderr_line.strip()}")

        # Recommendations
        if report["recommendations"]:
            print(f"\nRecommendations:")
            for rec in report["recommendations"]:
                print(f"  {rec}")

        print("\n" + "=" * 60)


def main():
    parser = argparse.ArgumentParser(
        description="Run veri compatibility tests across different environments"
    )
    parser.add_argument(
        "--veri-binary", default="veri", help="Path to veri binary (default: veri)"
    )
    parser.add_argument("--output", help="Output file for JSON report")
    parser.add_argument(
        "--test-dir",
        help="Use existing test directory instead of creating temporary one",
    )
    parser.add_argument(
        "--keep-test-dir",
        action="store_true",
        help="Keep test directory after completion (only with --test-dir)",
    )

    args = parser.parse_args()

    runner = MatrixTestRunner(args.veri_binary)

    # Create or use test directory
    if args.test_dir:
        test_dir = Path(args.test_dir)
        test_dir.mkdir(exist_ok=True)
        cleanup_dir = False
    else:
        temp_dir = tempfile.TemporaryDirectory()
        test_dir = Path(temp_dir.name)
        cleanup_dir = True

    try:
        print(f"Creating test project in {test_dir}...")
        runner.create_test_project(test_dir)

        print(f"Environment: Python {sys.version.split()[0]} on {platform.platform()}")

        # Run tests
        results = runner.run_all_tests(test_dir)

        # Generate and print report
        report = runner.generate_report(results)
        runner.print_report(report)

        # Save JSON report if requested
        if args.output:
            with open(args.output, "w") as f:
                json.dump(report, f, indent=2, default=str)
            print(f"\nDetailed report saved to: {args.output}")

        # Exit with appropriate code
        if report["summary"]["failed"] > 0:
            sys.exit(1)
        else:
            sys.exit(0)

    finally:
        if cleanup_dir and not args.keep_test_dir:
            # temp_dir will be cleaned up automatically
            pass
        elif args.keep_test_dir:
            print(f"\nTest directory preserved: {test_dir}")


if __name__ == "__main__":
    main()
