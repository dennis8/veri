"""Tests for cross-platform path handling."""

import os

import pytest

from veri_worker import VeriCollector


class TestPathHandling:
    """Test cases for cross-platform path handling."""

    def test_path_normalization_basic(self, temp_work_dir, temp_cache_dir):
        """Test basic path normalization."""
        collector = VeriCollector(temp_work_dir, temp_cache_dir)

        # Create nested test structure
        nested_dir = temp_work_dir / "tests" / "unit"
        nested_dir.mkdir(parents=True, exist_ok=True)

        test_file = nested_dir / "test_nested.py"
        test_file.write_text("""
def test_nested():
    assert True
""")

        result = collector.collect_tests()

        # All paths should be relative and use forward slashes for consistency
        for test in result["tests"]:
            path = test["path"]
            assert not os.path.isabs(path), f"Path should be relative: {path}"
            # On Windows, ensure paths are normalized
            if os.sep == "\\":
                assert "\\" not in path or "/" in path, (
                    f"Windows path should be normalized: {path}"
                )

    @pytest.mark.skipif(os.name != "nt", reason="Windows-specific test")
    def test_windows_drive_letter_handling(self, temp_work_dir, temp_cache_dir):
        """Test Windows drive letter handling."""
        collector = VeriCollector(temp_work_dir, temp_cache_dir)

        # Create test file
        test_file = temp_work_dir / "test_windows.py"
        test_file.write_text("""
def test_windows_path():
    assert True
""")

        result = collector.collect_tests()

        # Paths should not contain drive letters when made relative
        for test in result["tests"]:
            path = test["path"]
            assert ":" not in path, f"Path should not contain drive letter: {path}"

    @pytest.mark.skipif(os.name != "nt", reason="Windows-specific test")
    def test_windows_backslash_normalization(self, temp_work_dir, temp_cache_dir):
        """Test Windows backslash normalization."""
        # Create test with backslashes in path
        subdir = temp_work_dir / "sub\\dir"  # Deliberate backslash
        subdir.mkdir(parents=True, exist_ok=True)

        test_file = subdir / "test_backslash.py"
        test_file.write_text("""
def test_backslash():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Check that paths are properly normalized
        for test in result["tests"]:
            path = test["path"]
            module = test["module"]

            # Module names should use dots, not backslashes
            assert "\\" not in module, f"Module should use dots: {module}"
            # Paths should be consistently separated
            assert path.replace("\\", "/") == path or "\\" not in path

    def test_relative_path_calculation(self, temp_work_dir, temp_cache_dir):
        """Test relative path calculation."""
        # Create deeply nested structure
        deep_dir = temp_work_dir / "a" / "b" / "c" / "d"
        deep_dir.mkdir(parents=True, exist_ok=True)

        test_file = deep_dir / "test_deep.py"
        test_file.write_text("""
def test_deep():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Should find the nested test
        assert len(result["tests"]) >= 1

        test = result["tests"][0]
        path = test["path"]

        # Path should be relative and point to correct location
        full_path = temp_work_dir / path
        assert full_path.exists(), f"Computed path should exist: {full_path}"

    def test_symlink_handling(self, temp_work_dir, temp_cache_dir):
        """Test symlink handling (if supported)."""
        # Create test file
        test_file = temp_work_dir / "test_original.py"
        test_file.write_text("""
def test_original():
    assert True
""")

        # Try to create symlink (skip if not supported)
        link_file = temp_work_dir / "test_link.py"
        try:
            link_file.symlink_to(test_file)
        except (OSError, NotImplementedError):
            pytest.skip("Symlinks not supported on this platform")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Should handle symlinks gracefully (may collect both or resolve)
        assert len(result["tests"]) >= 1

    def test_unicode_path_handling(self, temp_work_dir, temp_cache_dir):
        """Test handling of unicode characters in paths."""
        # Create directory with unicode name
        unicode_dir = temp_work_dir / "测试"  # Chinese characters
        try:
            unicode_dir.mkdir(exist_ok=True)
        except (OSError, UnicodeError):
            pytest.skip("Unicode paths not supported on this filesystem")

        test_file = unicode_dir / "test_unicode.py"
        test_file.write_text("""
def test_unicode():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Should handle unicode paths
        assert len(result["tests"]) >= 1

        # Path should be properly encoded
        test = result["tests"][0]
        path = test["path"]
        assert isinstance(path, str)

    def test_long_path_handling(self, temp_work_dir, temp_cache_dir):
        """Test handling of long paths."""
        # Create a reasonably long path (not extreme, but longer than typical)
        long_path_parts = [
            "very",
            "long",
            "nested",
            "directory",
            "structure",
            "for",
            "testing",
        ]
        long_dir = temp_work_dir
        for part in long_path_parts:
            long_dir = long_dir / part

        try:
            long_dir.mkdir(parents=True, exist_ok=True)
        except OSError:
            pytest.skip("Cannot create long path on this filesystem")

        test_file = long_dir / "test_long_path.py"
        test_file.write_text("""
def test_long_path():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Should handle long paths
        assert len(result["tests"]) >= 1

    def test_case_sensitivity_handling(self, temp_work_dir, temp_cache_dir):
        """Test case sensitivity handling across platforms."""
        # Create test file
        test_file = temp_work_dir / "Test_Case.py"
        test_file.write_text("""
def test_case():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Should find the test regardless of case sensitivity
        assert len(result["tests"]) >= 1

        test = result["tests"][0]
        path = test["path"]

        # Path should preserve original case
        assert "Test_Case.py" in path
