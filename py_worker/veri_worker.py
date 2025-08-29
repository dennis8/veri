"""
veri Python worker - pytest compatibility shim
Handles test collection and execution via pytest integration
"""

import sys
import json
import argparse
import subprocess
from pathlib import Path
from datetime import datetime
from typing import Dict, List, Optional, Any
import pytest
from _pytest.config import Config
from _pytest.main import Session
from _pytest.nodes import Item
from _pytest.reports import TestReport


class VeriCollector:
    """Handles pytest collection and metadata extraction"""
    
    def __init__(self, work_dir: Path, cache_dir: Path):
        self.work_dir = work_dir
        self.cache_dir = cache_dir
        self.cache_dir.mkdir(parents=True, exist_ok=True)
    
    def collect_tests(self, paths: List[str] = None) -> Dict[str, Any]:
        """
        Use pytest to collect tests and extract metadata
        Returns collected test information in tests.index schema format
        """
        # Configure pytest for collection only
        args = ['--collect-only', '--quiet']
        if paths:
            args.extend(paths)
        
        # Capture pytest collection output
        collected_items = []
        collection_errors = []
        
        class CollectionPlugin:
            def pytest_collection_modifyitems(self, session, config, items):
                collected_items.extend(items)
            
            def pytest_collectreport(self, report):
                if report.failed:
                    collection_errors.append({
                        'path': str(report.nodeid.split('::')[0]) if '::' in report.nodeid else str(report.nodeid),
                        'line': None,
                        'error_type': type(report.longrepr).__name__ if hasattr(report, 'longrepr') else 'CollectionError',
                        'message': str(report.longrepr) if hasattr(report, 'longrepr') else 'Unknown collection error'
                    })
        
        # Run pytest collection
        plugin = CollectionPlugin()
        pytest.main(args, plugins=[plugin])
        
        # Extract test metadata
        tests = []
        for item in collected_items:
            test_info = self._extract_test_info(item)
            if test_info:
                tests.append(test_info)
        
        # Build index structure
        index_data = {
            'version': '0.1.0',
            'generated_at': datetime.utcnow().isoformat() + 'Z',
            'python_version': f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}",
            'pytest_version': pytest.__version__,
            'tests': tests,
            'collection_errors': collection_errors
        }
        
        return index_data
    
    def _extract_test_info(self, item: Item) -> Optional[Dict[str, Any]]:
        """Extract test metadata from pytest Item"""
        try:
            # Get file path relative to work directory - handle both old and new pytest versions
            if hasattr(item, 'path'):
                # New pytest versions use .path (pathlib.Path)
                file_path = item.path
            elif hasattr(item, 'fspath'):
                # Older pytest versions use .fspath
                file_path = Path(str(item.fspath))
            else:
                # Fallback
                file_path = Path(item.nodeid.split('::')[0])
            
            # Ensure work_dir is absolute for proper relative path calculation
            work_dir_abs = Path(self.work_dir).resolve()
            file_path_abs = file_path.resolve()
            
            try:
                rel_path = file_path_abs.relative_to(work_dir_abs)
            except ValueError:
                # If we can't make it relative, try using the nodeid file part
                rel_path = Path(item.nodeid.split('::')[0])
            
            # Extract markers
            markers = [mark.name for mark in item.iter_markers()]
            
            # Extract fixtures (from function signature)
            fixtures = []
            if hasattr(item, 'fixturenames'):
                fixtures = list(item.fixturenames)
            
            # Extract parametrization info if present
            parametrize = None
            if hasattr(item, 'callspec'):
                parametrize = {
                    'params': list(item.callspec.params.keys()) if hasattr(item.callspec, 'params') else [],
                    'ids': [item.callspec.id] if hasattr(item.callspec, 'id') else []
                }
            
            # Parse nodeid parts
            nodeid_parts = item.nodeid.split('::')
            file_part = nodeid_parts[0]
            function_part = nodeid_parts[-1]
            class_part = nodeid_parts[1] if len(nodeid_parts) > 2 else None
            
            # Extract module path
            module_path = str(rel_path).replace('/', '.').replace('\\', '.').replace('.py', '')
            
            return {
                'nodeid': item.nodeid,
                'path': str(rel_path),
                'line': item.location[1] + 1 if item.location else 1,  # pytest uses 0-based lines
                'function': function_part.split('[')[0],  # Remove parametrization suffix
                'class': class_part,
                'module': module_path,
                'markers': markers,
                'fixtures': fixtures,
                'parametrize': parametrize
            }
        except Exception as e:
            print(f"Warning: Failed to extract info for {item.nodeid}: {e}", file=sys.stderr)
            return None
    
    def collect_markers(self, tests_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Extract marker information from collected tests
        Returns marker index in markers.index schema format
        """
        markers_info = {}
        test_markers = {}
        
        # Analyze markers from tests
        for test in tests_data['tests']:
            nodeid = test['nodeid']
            test_markers[nodeid] = test['markers']
            
            for marker_name in test['markers']:
                if marker_name not in markers_info:
                    markers_info[marker_name] = {
                        'name': marker_name,
                        'description': None,
                        'registered': False,  # Would need pytest config to determine this
                        'usage_count': 0,
                        'first_seen': test['path'],
                        'common_args': []
                    }
                markers_info[marker_name]['usage_count'] += 1
        
        # Build markers index
        markers_data = {
            'version': '0.1.0',
            'generated_at': datetime.utcnow().isoformat() + 'Z',
            'markers': markers_info,
            'test_markers': test_markers
        }
        
        return markers_data
    
    def save_index(self, data: Dict[str, Any], filename: str):
        """Save index data to cache directory"""
        index_path = self.cache_dir / filename
        with open(index_path, 'w') as f:
            json.dump(data, f, indent=2)
        print(f"Saved {filename} to {index_path}")


class VeriExecutor:
    """Handles test execution via pytest"""
    
    def __init__(self, work_dir: Path):
        self.work_dir = work_dir
    
    def run_tests(self, nodeids: List[str], **kwargs) -> int:
        """
        Execute specific tests by nodeid
        Returns pytest exit code
        """
        args = []
        
        # Add nodeids to run
        args.extend(nodeids)
        
        # Add common pytest args based on kwargs
        if kwargs.get('verbose'):
            args.append('-v')
        if kwargs.get('quiet'):
            args.append('-q')
        if kwargs.get('no_capture'):
            args.append('-s')
        if kwargs.get('exitfirst'):
            args.append('-x')
        if maxfail := kwargs.get('maxfail'):
            args.extend(['--maxfail', str(maxfail)])
        if junit_xml := kwargs.get('junit_xml'):
            args.extend(['--junit-xml', str(junit_xml)])
        if workers := kwargs.get('workers'):
            if workers != '1':
                args.extend(['-n', str(workers)])
        
        # Run pytest
        return pytest.main(args)
    
    def run_pytest_engine(self, original_args: List[str]) -> int:
        """
        Hand off completely to pytest (--engine pytest mode)
        """
        # Filter out veri-specific args and pass the rest to pytest
        pytest_args = []
        skip_next = False
        
        for i, arg in enumerate(original_args):
            if skip_next:
                skip_next = False
                continue
                
            if arg in ['--engine', '--explain']:
                if arg == '--engine' and i + 1 < len(original_args):
                    skip_next = True
                continue
            
            # Convert veri args to pytest equivalents
            if arg == '--workers':
                if i + 1 < len(original_args) and original_args[i + 1] != '1':
                    pytest_args.extend(['-n', original_args[i + 1]])
                skip_next = True
            elif arg == '--no-capture':
                pytest_args.append('-s')
            elif arg in ['-a', '--all']:
                # pytest doesn't need explicit "all" flag
                continue
            else:
                pytest_args.append(arg)
        
        return pytest.main(pytest_args)


def main():
    parser = argparse.ArgumentParser(description='veri Python worker - pytest compatibility shim')
    parser.add_argument('command', choices=['collect', 'run', 'pytest-engine'], 
                       help='Command to execute')
    parser.add_argument('--work-dir', type=Path, default=Path.cwd(),
                       help='Working directory (default: current)')
    parser.add_argument('--cache-dir', type=Path, default=Path.cwd() / '.veri' / 'cache',
                       help='Cache directory for storing indexes')
    parser.add_argument('--paths', nargs='*', default=[],
                       help='Test paths or patterns')
    parser.add_argument('--nodeids', nargs='*', default=[],
                       help='Specific test nodeids to run')
    parser.add_argument('--verbose', action='store_true',
                       help='Verbose output')
    parser.add_argument('--quiet', action='store_true',
                       help='Quiet output')
    parser.add_argument('--no-capture', action='store_true',
                       help='Disable output capture')
    parser.add_argument('--exitfirst', action='store_true',
                       help='Exit after first failure')
    parser.add_argument('--maxfail', type=int,
                       help='Stop after N failures')
    parser.add_argument('--junit-xml', type=Path,
                       help='JUnit XML output path')
    parser.add_argument('--workers', default='1',
                       help='Number of workers for parallel execution')
    parser.add_argument('--pytest-args', nargs='*', default=[],
                       help='Additional arguments to pass to pytest')
    
    args = parser.parse_args()
    
    try:
        if args.command == 'collect':
            # Collection mode - generate tests.index and markers.index
            collector = VeriCollector(args.work_dir, args.cache_dir)
            
            print(f"Collecting tests from {args.work_dir}")
            if args.paths:
                print(f"Paths: {args.paths}")
            
            # Collect tests
            tests_data = collector.collect_tests(args.paths if args.paths else None)
            collector.save_index(tests_data, 'tests.index.json')
            
            # Collect markers
            markers_data = collector.collect_markers(tests_data)
            collector.save_index(markers_data, 'markers.index.json')
            
            print(f"Collected {len(tests_data['tests'])} tests")
            if tests_data['collection_errors']:
                print(f"Warning: {len(tests_data['collection_errors'])} collection errors")
                return 2
            
            return 0
            
        elif args.command == 'run':
            # Execution mode - run specific nodeids
            if not args.nodeids:
                print("Error: No nodeids specified for run command", file=sys.stderr)
                return 4
            
            executor = VeriExecutor(args.work_dir)
            
            print(f"Running {len(args.nodeids)} tests")
            if args.verbose:
                print(f"Nodeids: {args.nodeids}")
            
            exit_code = executor.run_tests(
                args.nodeids,
                verbose=args.verbose,
                quiet=args.quiet,
                no_capture=args.no_capture,
                exitfirst=args.exitfirst,
                maxfail=args.maxfail,
                junit_xml=args.junit_xml,
                workers=args.workers
            )
            
            return exit_code
            
        elif args.command == 'pytest-engine':
            # pytest engine mode - complete handoff
            executor = VeriExecutor(args.work_dir)
            pytest_args = args.pytest_args + args.paths
            
            print("Handing off to pytest engine")
            if args.verbose:
                print(f"pytest args: {pytest_args}")
            
            return executor.run_pytest_engine(pytest_args)
            
        else:
            print(f"Unknown command: {args.command}", file=sys.stderr)
            return 4
            
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    sys.exit(main())
