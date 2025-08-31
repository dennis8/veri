"""Tests for pytest collection functionality."""

import json

from veri_worker import VeriCollector


class TestVeriCollector:
    """Test cases for VeriCollector."""

    def test_collector_initialization(self, temp_work_dir, temp_cache_dir):
        """Test collector initialization."""
        collector = VeriCollector(temp_work_dir, temp_cache_dir)

        assert collector.work_dir == temp_work_dir
        assert collector.cache_dir == temp_cache_dir

    def test_collect_tests_empty_directory(self, temp_work_dir, temp_cache_dir):
        """Test collection with no test files."""
        collector = VeriCollector(temp_work_dir, temp_cache_dir)

        result = collector.collect_tests()

        assert "tests" in result
        assert "pytest_version" in result
        assert len(result["tests"]) == 0
        assert result["tests"] == []

    def test_collect_tests_with_simple_test(self, temp_work_dir, temp_cache_dir):
        """Test collection with a simple test file."""
        # Create a simple test file
        test_file = temp_work_dir / "test_simple.py"
        test_file.write_text("""
def test_addition():
    assert 1 + 1 == 2

def test_subtraction():
    assert 5 - 3 == 2

class TestCalculator:
    def test_multiply(self):
        assert 2 * 3 == 6
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        assert len(result["tests"]) >= 3

        # Check test info structure
        tests = result["tests"]
        assert len(tests) >= 3

        # Verify test info contains required fields
        for test in tests:
            assert "nodeid" in test
            assert "path" in test
            assert "function" in test
            assert "line" in test
            assert "module" in test
            assert "markers" in test
            assert "fixtures" in test

    def test_collect_tests_with_markers(self, temp_work_dir, temp_cache_dir):
        """Test collection with pytest markers."""
        test_file = temp_work_dir / "test_markers.py"
        test_file.write_text("""
import pytest

@pytest.mark.slow
def test_slow_operation():
    assert True

@pytest.mark.integration
@pytest.mark.slow
def test_integration():
    assert True

def test_fast():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        tests = result["tests"]

        # Find the marked tests
        slow_tests = [t for t in tests if "slow" in t["markers"]]
        integration_tests = [t for t in tests if "integration" in t["markers"]]
        unmarked_tests = [t for t in tests if not t["markers"]]

        assert len(slow_tests) >= 2
        assert len(integration_tests) >= 1
        assert len(unmarked_tests) >= 1

    def test_collect_tests_with_fixtures(self, temp_work_dir, temp_cache_dir):
        """Test collection with pytest fixtures."""
        # Create conftest with fixtures
        conftest_file = temp_work_dir / "conftest.py"
        conftest_file.write_text("""
import pytest

@pytest.fixture
def sample_data():
    return {"key": "value"}
""")

        test_file = temp_work_dir / "test_fixtures.py"
        test_file.write_text("""
def test_with_fixture(sample_data):
    assert sample_data["key"] == "value"

def test_no_fixture():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        tests = result["tests"]

        # Find tests with and without fixtures
        fixture_tests = [t for t in tests if t["fixtures"]]
        no_fixture_tests = [t for t in tests if not t["fixtures"]]

        assert len(fixture_tests) >= 1
        assert len(no_fixture_tests) >= 1

        # Check fixture names
        fixture_test = fixture_tests[0]
        assert "sample_data" in fixture_test["fixtures"]

    def test_collect_tests_with_parametrization(self, temp_work_dir, temp_cache_dir):
        """Test collection with parametrized tests."""
        test_file = temp_work_dir / "test_parametrize.py"
        test_file.write_text("""
import pytest

@pytest.mark.parametrize("input,expected", [
    (1, 2),
    (2, 3),
    (3, 4),
])
def test_increment(input, expected):
    assert input + 1 == expected
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        tests = result["tests"]

        # Should have 3 parametrized test instances
        parametrized_tests = [t for t in tests if t.get("parametrize")]
        assert len(parametrized_tests) >= 3

    def test_collect_markers_index(self, temp_work_dir, temp_cache_dir):
        """Test marker index collection."""
        test_file = temp_work_dir / "test_marker_index.py"
        test_file.write_text("""
import pytest

@pytest.mark.unit
def test_unit():
    assert True

@pytest.mark.integration
def test_integration():
    assert True

@pytest.mark.slow
@pytest.mark.integration
def test_slow_integration():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        tests_result = collector.collect_tests()
        markers_result = collector.collect_markers(tests_result)

        assert "version" in markers_result
        assert "markers" in markers_result
        assert "test_markers" in markers_result

        marker_defs = markers_result["markers"]
        test_markers = markers_result["test_markers"]

        # Should have collected markers
        assert "unit" in marker_defs
        assert "integration" in marker_defs
        assert "slow" in marker_defs

        # Test markers should map nodeids to marker lists
        assert len(test_markers) >= 3

    def test_save_and_load_index(self, temp_work_dir, temp_cache_dir):
        """Test saving and loading test index."""
        test_file = temp_work_dir / "test_save_load.py"
        test_file.write_text("""
def test_sample():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Save the index
        collector.save_index(result, "test_index.json")

        # Verify file was created
        index_file = temp_cache_dir / "test_index.json"
        assert index_file.exists()

        # Load and verify content
        with open(index_file) as f:
            loaded_data = json.load(f)

        assert len(loaded_data["tests"]) == len(result["tests"])
        assert len(loaded_data["tests"]) == len(result["tests"])
