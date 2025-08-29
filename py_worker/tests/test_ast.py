"""Tests for AST import parsing functionality."""



from veri_worker import VeriASTParser


class TestVeriASTParser:
    """Test cases for VeriASTParser."""

    def test_parser_initialization(self, temp_work_dir, sample_module_map):
        """Test parser initialization."""
        parser = VeriASTParser(temp_work_dir, sample_module_map)

        assert parser.work_dir == temp_work_dir
        assert parser.module_map == sample_module_map
        assert 'sys' in parser.builtin_modules

    def test_parse_basic_imports(self, temp_work_dir, sample_module_map, sample_python_file):
        """Test parsing basic import statements."""
        parser = VeriASTParser(temp_work_dir, sample_module_map)

        # Update module map to include our sample file
        rel_path = sample_python_file.relative_to(temp_work_dir)
        sample_module_map['modules'][str(rel_path)] = {
            'module_name': 'src.calculator',
            'path': str(rel_path)
        }

        imports_graph = parser.parse_imports_from_files()

        assert 'edges' in imports_graph
        assert 'dynamic_imports' in imports_graph
        assert 'unresolved_imports' in imports_graph
        assert imports_graph['version'] == '0.1.0'

        # Should find imports like 'math', 'typing', 'os' in unresolved imports
        unresolved = imports_graph['unresolved_imports']
        unresolved_modules = {imp['import_name'] for imp in unresolved}

        assert 'math' in unresolved_modules
        assert 'typing' in unresolved_modules
        assert 'os' in unresolved_modules

    def test_relative_import_resolution(self, temp_work_dir, sample_module_map):
        """Test relative import resolution."""
        parser = VeriASTParser(temp_work_dir, sample_module_map)

        # Test relative import resolution
        result = parser._resolve_relative_import('src.submodule.test', 'utils', 1)
        assert result == 'src.submodule.utils'

        result = parser._resolve_relative_import('src.submodule.test', 'utils', 2)
        assert result == 'src.utils'

    def test_dynamic_import_detection(self, temp_work_dir):
        """Test detection of dynamic imports."""
        # Create a file with dynamic imports
        dynamic_file = temp_work_dir / "dynamic.py"
        dynamic_file.write_text('''
import importlib

def load_plugin(name):
    module = importlib.import_module(f"plugins.{name}")
    return module

def load_dynamic():
    __import__("some.module")
''')

        module_map = {
            'modules': {
                'dynamic.py': {
                    'module_name': 'dynamic',
                    'path': 'dynamic.py'
                }
            }
        }

        parser = VeriASTParser(temp_work_dir, module_map)
        imports_graph = parser.parse_imports_from_files()

        dynamic_imports = imports_graph['dynamic_imports']
        assert len(dynamic_imports) >= 1

        # Should detect importlib usage
        dynamic_reasons = [di['reason'] for di in dynamic_imports]
        assert any('importlib' in reason for reason in dynamic_reasons)

    def test_builtin_module_detection(self, temp_work_dir, sample_module_map):
        """Test builtin module detection."""
        parser = VeriASTParser(temp_work_dir, sample_module_map)

        assert parser._is_builtin_module('sys')
        assert parser._is_builtin_module('os')
        assert not parser._is_builtin_module('numpy')
        assert not parser._is_builtin_module('custom_module')

    def test_local_module_detection(self, temp_work_dir, sample_module_map):
        """Test local module detection."""
        parser = VeriASTParser(temp_work_dir, sample_module_map)

        # Test with modules in the module map
        assert parser._is_local_module('src.calculator')
        assert parser._is_local_module('tests.test_calculator')
        assert not parser._is_local_module('external_package')

    def test_parse_file_with_syntax_error(self, temp_work_dir, sample_module_map):
        """Test handling files with syntax errors."""
        # Create a file with syntax error
        bad_file = temp_work_dir / "bad_syntax.py"
        bad_file.write_text('def incomplete_function(\n')

        module_map = {
            'modules': {
                'bad_syntax.py': {
                    'module_name': 'bad_syntax',
                    'path': 'bad_syntax.py'
                }
            }
        }

        parser = VeriASTParser(temp_work_dir, module_map)

        # Should not crash, should handle gracefully
        imports_graph = parser.parse_imports_from_files()
        assert 'edges' in imports_graph
        # No edges should be found from the bad file
        edges_from_bad = [e for e in imports_graph['edges'] if e['from_module'] == 'bad_syntax']
        assert len(edges_from_bad) == 0

    def test_empty_module_map(self, temp_work_dir):
        """Test with empty module map."""
        empty_map = {'modules': {}}
        parser = VeriASTParser(temp_work_dir, empty_map)

        imports_graph = parser.parse_imports_from_files()

        assert imports_graph['edges'] == []
        assert imports_graph['dynamic_imports'] == []
        assert imports_graph['unresolved_imports'] == []
