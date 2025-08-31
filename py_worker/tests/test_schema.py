"""Tests for schema validation and conformance."""

import json

import pytest

try:
    from jsonschema import ValidationError  # noqa: F401

    JSONSCHEMA_AVAILABLE = True
except ImportError:
    JSONSCHEMA_AVAILABLE = False

from veri_worker import VeriASTParser, VeriCollector


class TestSchemaValidation:
    """Test cases for JSON schema validation."""

    @pytest.mark.skipif(not JSONSCHEMA_AVAILABLE, reason="jsonschema not available")
    def test_imports_graph_schema_compliance(
        self, temp_work_dir, sample_module_map, sample_python_file
    ):
        """Test that imports graph conforms to expected schema."""
        parser = VeriASTParser(temp_work_dir, sample_module_map)

        # Update module map to include our sample file
        rel_path = sample_python_file.relative_to(temp_work_dir)
        sample_module_map["modules"][str(rel_path)] = {
            "module_name": "src.calculator",
            "path": str(rel_path),
        }

        imports_graph = parser.parse_imports_from_files()

        # Validate basic structure
        assert "version" in imports_graph
        assert "generated_at" in imports_graph
        assert "edges" in imports_graph
        assert "dynamic_imports" in imports_graph
        assert "unresolved_imports" in imports_graph

        # Validate edges structure
        for edge in imports_graph["edges"]:
            assert "from_module" in edge
            assert "to_module" in edge
            assert "import_type" in edge
            assert isinstance(edge["from_module"], str)
            assert isinstance(edge["to_module"], str)
            assert edge["import_type"] in ["import", "from_import"]

        # Validate dynamic imports structure
        for dynamic in imports_graph["dynamic_imports"]:
            assert "from_module" in dynamic
            assert "reason" in dynamic
            assert isinstance(dynamic["from_module"], str)
            assert isinstance(dynamic["reason"], str)

    def test_tests_index_schema_compliance(self, temp_work_dir, temp_cache_dir):
        """Test that tests index conforms to expected schema."""
        # Create a test file
        test_file = temp_work_dir / "test_schema.py"
        test_file.write_text("""
import pytest

@pytest.mark.unit
def test_example():
    assert True

class TestClass:
    def test_method(self):
        assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()

        # Validate basic structure
        assert "version" in result
        assert "generated_at" in result
        assert "tests" in result
        assert "pytest_version" in result

        # Validate tests structure
        for test in result["tests"]:
            assert "nodeid" in test
            assert "path" in test
            assert "line" in test
            assert "function" in test
            assert "module" in test
            assert "markers" in test
            assert "fixtures" in test

            assert isinstance(test["nodeid"], str)
            assert isinstance(test["path"], str)
            assert isinstance(test["line"], int)
            assert isinstance(test["function"], str)
            assert isinstance(test["module"], str)
            assert isinstance(test["markers"], list)
            assert isinstance(test["fixtures"], list)

    def test_markers_index_schema_compliance(self, temp_work_dir, temp_cache_dir):
        """Test that markers index conforms to expected schema."""
        test_file = temp_work_dir / "test_markers_schema.py"
        test_file.write_text("""
import pytest

@pytest.mark.slow
@pytest.mark.integration
def test_marked():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        tests_result = collector.collect_tests()
        markers_result = collector.collect_markers(tests_result)

        # Validate basic structure
        assert "version" in markers_result
        assert "generated_at" in markers_result
        assert "markers" in markers_result
        assert "test_markers" in markers_result

        # Validate marker definitions
        marker_defs = markers_result["markers"]
        assert isinstance(marker_defs, dict)

        for marker_name, marker_info in marker_defs.items():
            assert isinstance(marker_name, str)
            assert isinstance(marker_info, dict)
            assert "usage_count" in marker_info
            assert isinstance(marker_info["usage_count"], int)

        # Validate test markers
        test_markers = markers_result["test_markers"]
        assert isinstance(test_markers, dict)

        for nodeid, markers in test_markers.items():
            assert isinstance(nodeid, str)
            assert isinstance(markers, list)
            for marker in markers:
                assert isinstance(marker, str)

    def test_round_trip_serialization(self, temp_work_dir, temp_cache_dir):
        """Test that data can be serialized and deserialized without corruption."""
        test_file = temp_work_dir / "test_round_trip.py"
        test_file.write_text("""
def test_round_trip():
    assert True
""")

        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        original_result = collector.collect_tests()

        # Serialize to JSON
        json_str = json.dumps(original_result, indent=2)

        # Deserialize back
        deserialized_result = json.loads(json_str)

        # Compare key fields
        assert len(deserialized_result["tests"]) == len(original_result["tests"])

        # Compare first test if any
        if original_result["tests"]:
            orig_test = original_result["tests"][0]
            deser_test = deserialized_result["tests"][0]

            assert deser_test["nodeid"] == orig_test["nodeid"]
            assert deser_test["path"] == orig_test["path"]
            assert deser_test["function"] == orig_test["function"]

    def test_empty_results_schema_compliance(self, temp_work_dir, temp_cache_dir):
        """Test schema compliance with empty results."""
        collector = VeriCollector(temp_work_dir, temp_cache_dir)
        result = collector.collect_tests()  # No test files

        # Should still have valid structure
        assert len(result["tests"]) == 0
        assert result["tests"] == []
        assert "version" in result
        assert "pytest_version" in result

    def test_malformed_data_handling(self, temp_work_dir):
        """Test handling of malformed or edge-case data."""
        # Test with empty module map
        empty_map = {"modules": {}}
        parser = VeriASTParser(temp_work_dir, empty_map)

        imports_graph = parser.parse_imports_from_files()

        # Should produce valid empty structure
        assert imports_graph["edges"] == []
        assert imports_graph["dynamic_imports"] == []
        assert imports_graph["unresolved_imports"] == []
        assert "version" in imports_graph
