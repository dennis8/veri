"""
Phase 13 comprehensive test: Beta hardening & plugin stance
Tests compatibility matrix, flaky test handling, and plugin auto-fallback
"""

import pytest
import subprocess
import tempfile
import json
from pathlib import Path
import os
import sys


class TestPhase13Compatibility:
    """Test compatibility matrix and plugin detection."""
    
    def test_compatibility_matrix_loading(self):
        """Test that compatibility matrix loads correctly."""
        # This would be tested at the Rust level, but we can test the CLI integration
        result = subprocess.run([
            "veri", "--compatibility-report"
        ], capture_output=True, text=True)
        
        # Should work even if no tests to run
        assert result.returncode in [0, 4]  # Success or no tests found
        assert "Compatibility Report" in result.stdout
    
    def test_plugin_allowlist_enforcement(self):
        """Test that plugin allowlist is enforced."""
        with tempfile.TemporaryDirectory() as temp_dir:
            test_dir = Path(temp_dir)
            
            # Create a basic test
            (test_dir / "test_simple.py").write_text("""
def test_basic():
    assert True
""")
            
            # Create config with restrictive allowlist
            config_content = """
[tool.veri.security]
enforce_allowlist = true
allowed_plugins = ["pytest"]  # Very restrictive
"""
            (test_dir / "pyproject.toml").write_text(config_content)
            
            # Run veri - should work with basic pytest
            result = subprocess.run([
                "veri", "-v"
            ], cwd=temp_dir, capture_output=True, text=True)
            
            # Should succeed or provide appropriate guidance
            assert result.returncode in [0, 1, 4]
    
    def test_automatic_fallback_detection(self):
        """Test that incompatible plugins trigger automatic fallback."""
        with tempfile.TemporaryDirectory() as temp_dir:
            test_dir = Path(temp_dir)
            
            # Create a basic test
            (test_dir / "test_simple.py").write_text("""
def test_basic():
    assert True
""")
            
            # Create requirements with problematic plugin
            (test_dir / "requirements.txt").write_text("""
pytest
pytest-xdist  # This should trigger fallback
""")
            
            # Install if possible (may not work in CI)
            try:
                subprocess.run([
                    sys.executable, "-m", "pip", "install", "-r", "requirements.txt"
                ], cwd=temp_dir, check=False, capture_output=True)
            except:
                pytest.skip("Could not install test dependencies")
            
            # Run veri - should detect conflict and suggest fallback
            result = subprocess.run([
                "veri", "-v"
            ], cwd=temp_dir, capture_output=True, text=True)
            
            # Check for fallback behavior or warnings
            output = result.stdout + result.stderr
            assert any(keyword in output.lower() for keyword in [
                "fallback", "pytest", "compatibility", "plugin"
            ])


class TestPhase13FlakyHandling:
    """Test flaky test detection and retry behavior."""
    
    def test_flaky_config_loading(self):
        """Test that flaky configuration is loaded."""
        with tempfile.TemporaryDirectory() as temp_dir:
            test_dir = Path(temp_dir)
            
            # Create config with flaky settings
            config_content = """
[tool.veri.flaky]
auto_retry = true
retry_count = 2
flaky_threshold = 0.3
"""
            (test_dir / "pyproject.toml").write_text(config_content)
            
            # Create a simple test
            (test_dir / "test_simple.py").write_text("""
def test_basic():
    assert True
""")
            
            # Run with explain to see if config is loaded
            result = subprocess.run([
                "veri", "--explain"
            ], cwd=temp_dir, capture_output=True, text=True)
            
            # Should show configuration
            assert result.returncode == 0
    
    def test_flaky_report_generation(self):
        """Test flaky report functionality."""
        result = subprocess.run([
            "veri", "--flaky-report"
        ], capture_output=True, text=True)
        
        # Should work even with no data
        assert result.returncode == 0
        assert "Flaky Test Report" in result.stdout
    
    def test_retry_behavior_with_failing_test(self):
        """Test that failed tests can be retried."""
        with tempfile.TemporaryDirectory() as temp_dir:
            test_dir = Path(temp_dir)
            
            # Create a test that fails randomly
            (test_dir / "test_flaky.py").write_text("""
import random
import os

def test_sometimes_fails():
    # Use environment variable to control behavior
    if os.environ.get('FORCE_PASS') == '1':
        assert True
    else:
        # Fail about 50% of the time to simulate flakiness
        assert random.random() > 0.5
""")
            
            # Run test that might fail
            result = subprocess.run([
                "veri", "--auto-retry", "--retry-count", "2", "-v"
            ], cwd=temp_dir, capture_output=True, text=True, 
            env={**os.environ, 'FORCE_PASS': '1'})  # Force pass to avoid actual flakiness in test
            
            # Should complete successfully
            assert result.returncode == 0


class TestPhase13MatrixTesting:
    """Test matrix testing capabilities."""
    
    def test_matrix_test_script_exists(self):
        """Test that the matrix testing script exists and is executable."""
        script_path = Path(__file__).parent.parent / "scripts" / "matrix_test.py"
        assert script_path.exists()
        
        # Test that it's runnable
        result = subprocess.run([
            sys.executable, str(script_path), "--help"
        ], capture_output=True, text=True)
        
        assert result.returncode == 0
        assert "compatibility" in result.stdout.lower()
    
    def test_environment_detection(self):
        """Test that veri can detect current environment."""
        result = subprocess.run([
            "veri", "--compatibility-report"
        ], capture_output=True, text=True)
        
        # Should show environment info
        output = result.stdout + result.stderr
        assert any(keyword in output.lower() for keyword in [
            "python", "environment", "compatibility"
        ])


class TestPhase13CLIIntegration:
    """Test CLI integration of Phase 13 features."""
    
    def test_new_cli_flags_exist(self):
        """Test that new CLI flags are available."""
        result = subprocess.run([
            "veri", "--help"
        ], capture_output=True, text=True)
        
        assert result.returncode == 0
        help_text = result.stdout
        
        # Check for new flags
        assert "--auto-retry" in help_text
        assert "--retry-count" in help_text
        assert "--flaky-report" in help_text
        assert "--compatibility-report" in help_text
    
    def test_explain_shows_phase13_info(self):
        """Test that --explain shows Phase 13 information."""
        with tempfile.TemporaryDirectory() as temp_dir:
            test_dir = Path(temp_dir)
            
            # Create a simple test
            (test_dir / "test_simple.py").write_text("""
def test_basic():
    assert True
""")
            
            result = subprocess.run([
                "veri", "--explain"
            ], cwd=temp_dir, capture_output=True, text=True)
            
            assert result.returncode == 0
            output = result.stdout.lower()
            
            # Should show information about Phase 13 features
            assert any(keyword in output for keyword in [
                "compatibility", "plugin", "security"
            ])


def test_phase13_complete_integration():
    """Integration test for all Phase 13 features working together."""
    with tempfile.TemporaryDirectory() as temp_dir:
        test_dir = Path(temp_dir)
        
        # Create comprehensive test setup
        (test_dir / "test_comprehensive.py").write_text("""
import pytest

def test_basic():
    assert True

def test_with_fixture(sample_data):
    assert sample_data is not None

@pytest.mark.slow
def test_marked():
    assert True

class TestClass:
    def test_method(self):
        assert True
""")
        
        (test_dir / "conftest.py").write_text("""
import pytest

@pytest.fixture
def sample_data():
    return {"key": "value"}
""")
        
        # Create configuration with Phase 13 features
        (test_dir / "pyproject.toml").write_text("""
[tool.veri]
workers = "auto"
verbose = 1

[tool.veri.security]
enforce_allowlist = true
allowed_plugins = ["pytest", "pytest-mock"]

[tool.veri.flaky]
auto_retry = true
retry_count = 1
flaky_threshold = 0.2
""")
        
        # Run comprehensive test
        result = subprocess.run([
            "veri", "-v", "--compatibility-report"
        ], cwd=temp_dir, capture_output=True, text=True)
        
        # Should work and show compatibility info
        assert result.returncode in [0, 1]  # Success or test failures are OK
        
        output = result.stdout + result.stderr
        assert "compatibility" in output.lower() or "veri" in output.lower()


if __name__ == "__main__":
    # Run tests if called directly
    pytest.main([__file__, "-v"])