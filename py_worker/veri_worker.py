"""
veri Python worker - pytest compatibility shim and AST import parser
Handles test collection/execution via pytest integration and import analysis
"""

import argparse
import ast
import io
import json
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pytest
from _pytest.nodes import Item

from contracts import (
    CollectionError,
    MarkerInfo,
    MarkersIndex,
    ParametrizeInfo,
    TestNode,
    TestsIndex,
)
from contracts.base import SchemaModel

try:
    import coverage

    COVERAGE_AVAILABLE = True
except ImportError:
    COVERAGE_AVAILABLE = False


class VeriASTParser:
    """Handles AST-based import analysis for building import graphs"""

    def __init__(self, work_dir: Path, module_map: dict[str, Any]) -> None:
        self.work_dir = work_dir
        self.module_map = module_map
        self.builtin_modules = set(sys.builtin_module_names)
        # Add common standard library modules
        self.stdlib_modules = self.builtin_modules | {
            "os",
            "sys",
            "math",
            "time",
            "datetime",
            "json",
            "urllib",
            "http",
            "threading",
            "multiprocessing",
            "subprocess",
            "pathlib",
            "shutil",
            "typing",
            "collections",
            "itertools",
            "functools",
            "operator",
            "logging",
            "re",
            "ast",
            "inspect",
            "importlib",
            "pkgutil",
            "unittest",
            "pytest",
            "argparse",
            "configparser",
            "sqlite3",
            "email",
            "xml",
            "html",
            "csv",
            "gzip",
            "zipfile",
            "tarfile",
        }

    def parse_imports_from_files(self) -> dict[str, Any]:
        """
        Parse imports from all Python files and build imports graph
        Returns import graph in imports.graph.json schema format
        """
        edges = []
        dynamic_imports = []
        unresolved_imports = []
        parse_errors = []

        # Process each module in the module map
        for file_path, module_info in self.module_map["modules"].items():
            try:
                full_path = self.work_dir / file_path
                if full_path.exists() and full_path.suffix == ".py":
                    file_edges, file_dynamic, file_unresolved = (
                        self._parse_file_imports(full_path, module_info["module_name"])
                    )
                    edges.extend(file_edges)
                    dynamic_imports.extend(file_dynamic)
                    unresolved_imports.extend(file_unresolved)
            except Exception as e:
                error_msg = f"Failed to parse imports from {file_path}: {e}"
                print(f"Warning: {error_msg}", file=sys.stderr)
                parse_errors.append(error_msg)

        # Warn if too many parse errors
        if parse_errors:
            total_modules = max(1, len(self.module_map.get("modules", {})))
            error_rate = len(parse_errors) / total_modules
            if error_rate > 0.1:  # More than 10% failure rate
                print(f"⚠️  High import parse error rate: {len(parse_errors)} files failed", file=sys.stderr)
                print("ℹ️  This may result in incomplete import graphs and reduced test impact accuracy", file=sys.stderr)

        return {
            "version": "0.1.0",
            "generated_at": datetime.now(UTC).isoformat() + "Z",
            "edges": edges,
            "dynamic_imports": dynamic_imports,
            "unresolved_imports": unresolved_imports,
        }

    def _parse_file_imports(
        self, file_path: Path, from_module: str
    ) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
        """Parse imports from a single Python file"""
        edges = []
        dynamic_imports = []
        unresolved_imports = []

        try:
            with open(file_path, encoding="utf-8") as f:
                content = f.read()

            tree = ast.parse(content, filename=str(file_path))

            # Track conditional context (inside if/try/except blocks)
            conditional_stack = []

            for node in ast.walk(tree):
                # Track conditional blocks for improved import analysis
                if isinstance(node, (ast.If, ast.Try, ast.ExceptHandler, ast.With)):
                    conditional_stack.append(node.lineno)

                # Parse import statements
                if isinstance(node, ast.Import):
                    for alias in node.names:
                        edge, unresolved = self._process_import(
                            from_module,
                            alias.name,
                            alias.asname,
                            node.lineno,
                            "import",
                            [],
                            len(conditional_stack) > 0,
                        )
                        if edge:
                            edges.append(edge)
                        elif unresolved:
                            unresolved_imports.append(unresolved)

                elif isinstance(node, ast.ImportFrom):
                    module_name = node.module or ""
                    level = node.level

                    # Handle relative imports
                    if level > 0:
                        resolved_module = self._resolve_relative_import(
                            from_module, module_name, level
                        )
                        import_type = "relative"
                    else:
                        resolved_module = module_name
                        import_type = "from"

                    names = [alias.name for alias in node.names] if node.names else []

                    edge, unresolved = self._process_import(
                        from_module,
                        resolved_module,
                        None,
                        node.lineno,
                        import_type,
                        names,
                        len(conditional_stack) > 0,
                    )
                    if edge:
                        edges.append(edge)
                    elif unresolved:
                        unresolved_imports.append(unresolved)

                # Detect dynamic imports
                elif isinstance(node, ast.Call):
                    dynamic_import = self._detect_dynamic_import(from_module, node)
                    if dynamic_import:
                        dynamic_imports.append(dynamic_import)

        except SyntaxError as e:
            print(f"Syntax error in {file_path}: {e}", file=sys.stderr)
            print(f"  This file's imports will not be included in the import graph", file=sys.stderr)
        except Exception as e:
            print(f"Error parsing {file_path}: {e}", file=sys.stderr)
            print(f"  This file's imports will not be included in the import graph", file=sys.stderr)

        return edges, dynamic_imports, unresolved_imports

    def _process_import(
        self,
        from_module: str,
        to_module: str,
        alias: str | None,
        line: int,
        import_type: str,
        names: list[str],
        is_conditional: bool,
    ) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
        """Process a single import and return edge or unresolved import"""

        if not to_module:
            return None, None

        # Check if this is a local module
        if self._is_local_module(to_module):
            return {
                "from_module": from_module,
                "to_module": to_module,
                "import_type": import_type,
                "line": line,
                "names": names,
                "alias": alias,
                "is_conditional": is_conditional,
            }, None
        else:
            # Unresolved import (third-party or builtin)
            return None, {
                "from_module": from_module,
                "import_name": to_module,
                "line": line,
                "is_third_party": not self._is_builtin_module(to_module),
                "is_builtin": self._is_builtin_module(to_module),
            }

    def _resolve_relative_import(
        self, from_module: str, module_name: str, level: int
    ) -> str:
        """Resolve relative import to absolute module name"""
        module_parts = from_module.split(".")

        # Go up 'level' number of packages
        if level > len(module_parts):
            # Invalid relative import - treat as the module name itself
            return module_name or from_module

        base_parts = module_parts[:-level] if level > 0 else module_parts

        if module_name:
            return ".".join(base_parts + [module_name])
        else:
            return ".".join(base_parts)

    def _is_local_module(self, module_name: str) -> bool:
        """Check if a module is local to this project"""
        # Check if module is in our module map
        for module_info in self.module_map["modules"].values():
            if module_info["module_name"] == module_name:
                return True

        # Check if it's a submodule of any local package
        for module_info in self.module_map["modules"].values():
            if module_name.startswith(module_info["module_name"] + "."):
                return True

        return False

    def _is_builtin_module(self, module_name: str) -> bool:
        """Check if a module is a Python builtin or standard library module"""
        return module_name.split(".")[0] in self.stdlib_modules

    def _detect_dynamic_import(
        self, from_module: str, node: ast.Call
    ) -> dict[str, Any] | None:
        """Detect dynamic import patterns"""
        # Check for importlib.import_module
        if (
            isinstance(node.func, ast.Attribute)
            and isinstance(node.func.value, ast.Name)
            and node.func.value.id == "importlib"
            and node.func.attr == "import_module"
        ):
            argument = None
            if node.args and isinstance(node.args[0], ast.Constant):
                argument = str(node.args[0].value)

            return {
                "from_module": from_module,
                "line": node.lineno,
                "function": "importlib.import_module",
                "argument": argument,
                "reason": "Dynamic module import via importlib.import_module",
            }

        # Check for __import__
        elif isinstance(node.func, ast.Name) and node.func.id == "__import__":
            argument = None
            if node.args and isinstance(node.args[0], ast.Constant):
                argument = str(node.args[0].value)

            return {
                "from_module": from_module,
                "line": node.lineno,
                "function": "__import__",
                "argument": argument,
                "reason": "Dynamic module import via __import__",
            }

        # Check for exec/eval with import-like patterns
        elif isinstance(node.func, ast.Name) and node.func.id in ["exec", "eval"]:
            return {
                "from_module": from_module,
                "line": node.lineno,
                "function": node.func.id,
                "argument": None,
                "reason": f"Potential dynamic import via {node.func.id}",
            }

        return None


class VeriCollector:
    """Handles pytest collection and metadata extraction"""

    def __init__(self, work_dir: Path, cache_dir: Path) -> None:
        self.work_dir = work_dir
        self.cache_dir = cache_dir
        self.cache_dir.mkdir(parents=True, exist_ok=True)

    def collect_tests(
        self, paths: list[str] | None = None, ignores: list[str] | None = None
    ) -> TestsIndex:
        """
        Use pytest to collect tests and extract metadata
        Returns collected test information in tests.index schema format
        """
        # Configure pytest for collection only
        args = ["--collect-only", "--quiet"]
        # Be permissive about test file patterns across platforms (case-insensitive variants)
        # pytest expects space-separated patterns for list options
        args.extend(["-o", "python_files=test*.py Test*.py"])
        if paths:
            args.extend(paths)
        # Add ignores
        if ignores:
            for ig in ignores:
                args.extend(["--ignore", ig])

        # Capture pytest collection output
        collected_items = []
        collection_errors: list[CollectionError] = []

        class CollectionPlugin:
            def pytest_collection_modifyitems(
                self, session: Any, config: Any, items: list[Item]
            ) -> None:
                collected_items.extend(items)

            def pytest_collectreport(self, report: Any) -> None:
                if report.failed:
                    path = (
                        str(report.nodeid.split("::")[0])
                        if "::" in report.nodeid
                        else str(report.nodeid)
                    )
                    error_type = (
                        type(report.longrepr).__name__
                        if hasattr(report, "longrepr")
                        else "CollectionError"
                    )
                    message = (
                        str(report.longrepr)
                        if hasattr(report, "longrepr")
                        else "Unknown collection error"
                    )
                    collection_errors.append(
                        CollectionError(
                            path=path,
                            line=None,
                            error_type=error_type,
                            message=message,
                        )
                    )

        # Run pytest collection from the correct working directory
        import os

        plugin = CollectionPlugin()
        original_cwd = os.getcwd()
        try:
            os.chdir(self.work_dir)
            pytest.main(args, plugins=[plugin])
        finally:
            os.chdir(original_cwd)

        # Extract test metadata
        tests: list[TestNode] = []
        for item in collected_items:
            test_info = self._extract_test_info(item)
            if test_info:
                tests.append(test_info)

        # Build index structure
        return TestsIndex(
            version="0.1.0",
            generated_at=datetime.now(UTC),
            python_version=(
                f"{sys.version_info.major}."
                f"{sys.version_info.minor}."
                f"{sys.version_info.micro}"
            ),
            pytest_version=pytest.__version__,
            tests=tests,
            collection_errors=collection_errors,
        )

    def _extract_test_info(self, item: Item) -> TestNode | None:
        """Extract test metadata from pytest Item"""
        try:
            # Get file path relative to work directory - handle both old and new pytest versions
            if hasattr(item, "path"):
                # New pytest versions use .path (pathlib.Path)
                file_path = item.path
            elif hasattr(item, "fspath"):
                # Older pytest versions use .fspath
                file_path = Path(str(item.fspath))
            else:
                # Fallback
                file_path = Path(item.nodeid.split("::")[0])

            # Ensure work_dir is absolute for proper relative path calculation
            work_dir_abs = Path(self.work_dir).resolve()
            file_path_abs = file_path.resolve()

            try:
                rel_path = file_path_abs.relative_to(work_dir_abs)
            except ValueError:
                # If we can't make it relative, try using the nodeid file part
                rel_path = Path(item.nodeid.split("::")[0])

            # Extract markers
            markers = [mark.name for mark in item.iter_markers()]

            # Extract fixtures (from function signature)
            fixtures = []
            if hasattr(item, "fixturenames"):
                fixtures = list(item.fixturenames)

            # Extract parametrization info if present
            parametrize = None
            if hasattr(item, "callspec"):
                params = (
                    list(item.callspec.params.keys())
                    if hasattr(item.callspec, "params")
                    else []
                )
                ids = [str(item.callspec.id)] if hasattr(item.callspec, "id") else []
                parametrize = ParametrizeInfo(params=params, ids=ids)

            # Parse nodeid parts
            nodeid_parts = item.nodeid.split("::")
            function_part = nodeid_parts[-1]
            class_part = nodeid_parts[1] if len(nodeid_parts) > 2 else None

            # Extract module path
            module_path = (
                str(rel_path).replace("/", ".").replace("\\", ".").replace(".py", "")
            )

            # Normalize path to use forward slashes consistently
            normalized_path = str(rel_path).replace("\\", "/")

            kwargs: dict[str, Any] = {
                "nodeid": item.nodeid,
                "path": normalized_path,
                "line": (
                    (item.location[1] + 1)
                    if (item.location and item.location[1] is not None)
                    else 1
                ),
                "function": function_part.split("[")[0],
                "class": class_part,
                "module": module_path,
                "markers": markers,
                "fixtures": fixtures,
                "parametrize": parametrize,
            }
            return TestNode(**kwargs)
        except Exception as e:
            print(
                f"Warning: Failed to extract info for {item.nodeid}: {e}",
                file=sys.stderr,
            )
            return None

    def collect_markers(self, tests_data: TestsIndex) -> MarkersIndex:
        """
        Extract marker information from collected tests
        Returns marker index in markers.index schema format
        """
        marker_accumulator: dict[str, dict[str, Any]] = {}
        test_markers: dict[str, list[str]] = {}

        # Analyze markers from tests
        for test in tests_data.tests:
            test_markers[test.nodeid] = list(test.markers)

            for marker_name in test.markers:
                marker_entry = marker_accumulator.setdefault(
                    marker_name,
                    {
                        "name": marker_name,
                        "description": None,
                        "registered": False,
                        "usage_count": 0,
                        "first_seen": test.path,
                        "common_args": [],
                    },
                )
                marker_entry["usage_count"] += 1

        markers = {
            name: MarkerInfo(**info) for name, info in marker_accumulator.items()
        }

        return MarkersIndex(
            version="0.1.0",
            generated_at=datetime.now(UTC),
            markers=markers,
            test_markers=test_markers,
        )

    def save_index(self, data: SchemaModel, filename: str) -> None:
        """Save index data to cache directory"""
        index_path = self.cache_dir / filename
        with open(index_path, "w", encoding="utf-8") as f:
            f.write(data.to_schema_json())
        print(f"Saved {filename} to {index_path}")


class VeriExecutor:
    """Handles test execution via pytest"""

    def __init__(self, work_dir: Path) -> None:
        self.work_dir = work_dir
        # Use a broad type to keep mypy happy when coverage may be unavailable
        self.coverage_instance: Any = None

    def run_tests(self, nodeids: list[str], **kwargs: Any) -> int:
        """
        Execute specific tests by nodeid
        Returns pytest exit code
        """
        # Early return for empty nodeids - nothing to run
        if not nodeids:
            return 0

        # Handle coverage if enabled
        if kwargs.get("coverage"):
            if not COVERAGE_AVAILABLE:
                print(
                    "Warning: coverage package not available, skipping coverage collection",
                    file=sys.stderr,
                )
            else:
                self._start_coverage(**kwargs)

        args = []

        # Add nodeids to run
        args.extend(nodeids)

        # Add common pytest args based on kwargs
        if kwargs.get("verbose"):
            args.append("-v")
        if kwargs.get("quiet"):
            args.append("-q")
        if kwargs.get("no_capture"):
            args.append("-s")
        if kwargs.get("exitfirst"):
            args.append("-x")
        if maxfail := kwargs.get("maxfail"):
            args.extend(["--maxfail", str(maxfail)])
        if junit_xml := kwargs.get("junit_xml"):
            args.extend(["--junit-xml", str(junit_xml)])
        if workers := kwargs.get("workers"):
            if workers != "1":
                args.extend(["-n", str(workers)])

        # Add ignores
        for ig in kwargs.get("ignore", []) or []:
            args.extend(["--ignore", ig])

        # Hook plugin to capture per-test durations/outcomes (including setup skips)
        class ExecPlugin:
            def __init__(self, sink: list[dict[str, Any]]):
                self._by_nodeid: dict[str, dict[str, Any]] = {}
                self._sink = sink

            def pytest_runtest_logreport(self, report: Any) -> None:
                nodeid = getattr(report, "nodeid", "")
                when = getattr(report, "when", "call")
                outcome = getattr(report, "outcome", "")
                duration_ms = int(getattr(report, "duration", 0.0) * 1000)

                def rank(o: str) -> int:
                    return {"failed": 3, "error": 3, "skipped": 2, "passed": 1}.get(
                        o, 0
                    )

                new_outcome = outcome
                if outcome == "failed" and when != "call":
                    new_outcome = "error"

                # Only record meaningful outcomes; prefer call-phase duration for passes
                if new_outcome == "passed" and when != "call":
                    return

                prev = self._by_nodeid.get(nodeid)
                entry = {
                    "nodeid": nodeid,
                    "outcome": new_outcome,
                    "duration_ms": duration_ms,
                }
                if prev is None or rank(new_outcome) >= rank(prev.get("outcome", "")):
                    self._by_nodeid[nodeid] = entry

            def finalize(self) -> None:
                self._sink.extend(self._by_nodeid.values())

        self.last_per_test: list[dict[str, Any]] = []
        plugin = ExecPlugin(self.last_per_test)

        # Run pytest from the correct working directory
        import os

        original_cwd = os.getcwd()
        try:
            os.chdir(self.work_dir)
            # Capture stdout/stderr if requested (for worker mode)
            capture_output = bool(kwargs.get("_capture_output", False))
            if capture_output:
                import io as _io
                from contextlib import redirect_stderr, redirect_stdout

                out_buf, err_buf = _io.StringIO(), _io.StringIO()
                with redirect_stdout(out_buf), redirect_stderr(err_buf):
                    exit_code = pytest.main(args, plugins=[plugin])
                self.last_stdout = out_buf.getvalue()
                self.last_stderr = err_buf.getvalue()
            else:
                exit_code = pytest.main(args, plugins=[plugin])

            # Stop coverage and save data
            if kwargs.get("coverage") and COVERAGE_AVAILABLE and self.coverage_instance:
                self._stop_coverage(**kwargs)

            # Flush plugin results
            try:
                plugin.finalize()
            except Exception:
                pass

            return exit_code
        finally:
            os.chdir(original_cwd)

    def _start_coverage(self, **kwargs: Any) -> None:
        """Initialize and start coverage collection"""
        if not COVERAGE_AVAILABLE:
            return

        # Create coverage instance with appropriate settings
        config_file = self.work_dir / ".coveragerc"
        if config_file.exists():
            self.coverage_instance = coverage.Coverage(config_file=str(config_file))
        else:
            # Use sensible defaults for incremental coverage
            source_dirs = kwargs.get("coverage_source_dirs", ["src"])
            omit_patterns = kwargs.get(
                "coverage_omit",
                ["*/tests/*", "*/test_*", "*/__pycache__/*", "*/venv/*", "*/.venv/*"],
            )

            # Use per-worker coverage data file if VERI_WORKER_ID is provided
            import os as _os

            worker_id = _os.environ.get("VERI_WORKER_ID")
            data_file = (
                self.work_dir
                / ".veri"
                / "cache"
                / (f".coverage.worker_{worker_id}" if worker_id else ".coverage")
            )

            self.coverage_instance = coverage.Coverage(
                source=source_dirs,
                omit=omit_patterns,
                branch=True,
                data_file=str(data_file),
            )

        # Start coverage measurement
        self.coverage_instance.start()
        print("Started coverage collection")

    def _stop_coverage(self, **kwargs: Any) -> None:
        """Stop coverage collection and generate reports"""
        if not self.coverage_instance:
            return

        # Stop coverage measurement
        self.coverage_instance.stop()
        self.coverage_instance.save()

        # Generate JSON report for veri to process
        cache_dir = self.work_dir / ".veri" / "cache"
        cache_dir.mkdir(parents=True, exist_ok=True)

        json_report_path = cache_dir / "coverage.json"

        try:
            # Generate JSON report (coverage.json_report expects a file path)
            self.coverage_instance.json_report(
                outfile=str(json_report_path), pretty_print=True
            )

            print(f"Coverage data saved to {json_report_path}")

            # Generate additional reports if requested
            if kwargs.get("coverage_xml"):
                xml_path = kwargs.get(
                    "coverage_xml_path", self.work_dir / "coverage.xml"
                )
                self.coverage_instance.xml_report(outfile=str(xml_path))
                print(f"Coverage XML report saved to {xml_path}")

            if kwargs.get("coverage_html"):
                html_dir = kwargs.get("coverage_html_dir", self.work_dir / "htmlcov")
                self.coverage_instance.html_report(directory=str(html_dir))
                print(f"Coverage HTML report saved to {html_dir}")

        except Exception as e:
            print(f"Error generating coverage reports: {e}", file=sys.stderr)

    def run_pytest_engine(self, original_args: list[str]) -> int:
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

            if arg in ["--engine", "--explain"]:
                if arg == "--engine" and i + 1 < len(original_args):
                    skip_next = True
                continue

            # Convert veri args to pytest equivalents
            if arg == "--workers":
                if i + 1 < len(original_args) and original_args[i + 1] != "1":
                    pytest_args.extend(["-n", original_args[i + 1]])
                skip_next = True
            elif arg == "--no-capture":
                pytest_args.append("-s")
            elif arg in ["-a", "--all"]:
                # pytest doesn't need explicit "all" flag
                continue
            else:
                pytest_args.append(arg)

        # Run pytest from the correct working directory
        import os

        original_cwd = os.getcwd()
        try:
            os.chdir(self.work_dir)
            return pytest.main(pytest_args)
        finally:
            os.chdir(original_cwd)


def run_worker_mode(args: Any) -> int:
    """Long‑lived worker JSONL protocol loop.

    Reads JSON commands from stdin and writes JSON responses to stdout.
    """

    stdin = io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8", newline="\n")
    stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", newline="\n")

    def send(obj: dict[str, Any]) -> None:
        stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
        stdout.flush()

    # Send HelloOk immediately (Rust may or may not send Hello first)
    send(
        {
            "t": "HelloOk",
            "worker_id": int(getattr(args, "worker_id", 0) or 0),
            "py_version": f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}",
            "pytest_version": getattr(pytest, "__version__", "unknown"),
        }
    )

    executor = VeriExecutor(args.work_dir)

    while True:
        line = stdin.readline()
        if line == "":
            # stdin closed
            break
        line = line.strip()
        if not line:
            continue

        try:
            msg = json.loads(line)
            t = msg.get("t")
            if t == "HealthCheck":
                send({"t": "HealthOk", "ts": datetime.now(UTC).isoformat() + "Z"})
            elif t == "Shutdown":
                break
            elif t == "ExecuteTests":
                batch_id = msg.get("batch_id", "batch")
                nodeids = msg.get("nodeids", [])
                options = msg.get("options", {})

                # Map JSON options to executor args
                start = datetime.now(UTC)
                print(
                    f"[worker {getattr(args, 'worker_id', 0)}] ExecuteTests {batch_id}: {len(nodeids)} nodeids",
                    file=sys.stderr,
                )
                exit_code = executor.run_tests(
                    nodeids,
                    verbose=bool(options.get("verbose", False)),
                    quiet=bool(options.get("quiet", False)),
                    no_capture=bool(options.get("no_capture", False)),
                    exitfirst=bool(options.get("exitfirst", False)),
                    maxfail=options.get("maxfail"),
                    junit_xml=Path(options["junit_xml"])
                    if options.get("junit_xml")
                    else None,
                    workers=str(options.get("workers", "1")),
                    coverage=bool(options.get("coverage", False)),
                    coverage_xml=bool(options.get("coverage_xml", False)),
                    coverage_html=bool(options.get("coverage_html", False)),
                    coverage_source_dirs=list(
                        options.get("coverage_source_dirs", ["src"])
                    ),
                    coverage_omit=list(options.get("coverage_omit", [])),
                    _capture_output=True,
                )
                dur_ms = int((datetime.now(UTC) - start).total_seconds() * 1000)
                print(
                    f"[worker {getattr(args, 'worker_id', 0)}] Completed {batch_id} in {dur_ms}ms (exit {exit_code})",
                    file=sys.stderr,
                )

                # For now stdout/stderr are not captured live; provide empty strings
                send(
                    {
                        "t": "TestResults",
                        "batch_id": batch_id,
                        "exit_code": int(exit_code),
                        "stdout": executor.last_stdout or "",
                        "stderr": executor.last_stderr or "",
                        "duration_ms": dur_ms,
                        "nodeids": nodeids,
                        "per_test": executor.last_per_test,
                    }
                )
            else:
                send(
                    {
                        "t": "Error",
                        "kind": "UnknownMessage",
                        "message": f"Unknown t={t}",
                    }
                )
        except Exception as e:  # pragma: no cover - defensive
            send({"t": "Error", "kind": "Exception", "message": str(e)})

    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="veri Python worker - pytest compatibility shim and AST parser"
    )
    parser.add_argument(
        "--worker-mode",
        action="store_true",
        help="Start in long-lived worker mode (JSONL protocol)",
    )
    parser.add_argument(
        "--worker-id", type=int, default=0, help="Worker id in worker mode"
    )
    parser.add_argument(
        "command",
        nargs="?",
        choices=["collect", "run", "pytest-engine", "parse-imports"],
        help="Command to execute",
    )
    parser.add_argument(
        "--work-dir",
        type=Path,
        default=Path.cwd(),
        help="Working directory (default: current)",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=Path.cwd() / ".veri" / "cache",
        help="Cache directory for storing indexes",
    )
    parser.add_argument("--paths", nargs="*", default=[], help="Test paths or patterns")
    parser.add_argument(
        "--ignore", action="append", default=[], help="Ignore test path (repeatable)"
    )
    parser.add_argument(
        "--nodeids", nargs="*", default=[], help="Specific test nodeids to run"
    )
    parser.add_argument("--verbose", action="store_true", help="Verbose output")
    parser.add_argument("--quiet", action="store_true", help="Quiet output")
    parser.add_argument(
        "--no-capture", action="store_true", help="Disable output capture"
    )
    parser.add_argument(
        "--exitfirst", action="store_true", help="Exit after first failure"
    )
    parser.add_argument("--maxfail", type=int, help="Stop after N failures")
    parser.add_argument("--junit-xml", type=Path, help="JUnit XML output path")
    parser.add_argument(
        "--workers", default="1", help="Number of workers for parallel execution"
    )
    parser.add_argument(
        "--coverage", action="store_true", help="Enable coverage collection"
    )
    parser.add_argument(
        "--coverage-xml", action="store_true", help="Generate XML coverage report"
    )
    parser.add_argument(
        "--coverage-html", action="store_true", help="Generate HTML coverage report"
    )
    parser.add_argument(
        "--coverage-source-dirs",
        nargs="*",
        default=["src"],
        help="Source directories for coverage",
    )
    parser.add_argument(
        "--coverage-omit",
        nargs="*",
        default=["*/tests/*", "*/test_*", "*/__pycache__/*", "*/venv/*", "*/.venv/*"],
        help="Patterns to omit from coverage",
    )
    parser.add_argument(
        "--pytest-args",
        nargs="*",
        default=[],
        help="Additional arguments to pass to pytest",
    )
    parser.add_argument(
        "--module-map",
        type=Path,
        help="Path to module map JSON file (for parse-imports command)",
    )

    args = parser.parse_args()

    if args.worker_mode:
        return run_worker_mode(args)

    try:
        if args.command == "collect":
            # Collection mode - generate tests.index and markers.index
            collector = VeriCollector(args.work_dir, args.cache_dir)

            print(f"Collecting tests from {args.work_dir}")
            if args.paths:
                print(f"Paths: {args.paths}")

            # Collect tests
            tests_index = collector.collect_tests(
                args.paths if args.paths else None, ignores=args.ignore
            )
            collector.save_index(tests_index, "tests.index.json")

            # Collect markers
            markers_index = collector.collect_markers(tests_index)
            collector.save_index(markers_index, "markers.index.json")

            print(f"Collected {len(tests_index.tests)} tests")
            if tests_index.collection_errors:
                print(
                    f"Warning: {len(tests_index.collection_errors)} collection errors"
                )
                return 2

            return 0

        elif args.command == "parse-imports":
            # Import parsing mode - generate imports.graph.json
            if not args.module_map:
                print(
                    "Error: --module-map is required for parse-imports command",
                    file=sys.stderr,
                )
                return 4

            if not args.module_map.exists():
                print(
                    f"Error: Module map file not found: {args.module_map}",
                    file=sys.stderr,
                )
                return 4

            # Load module map
            with open(args.module_map) as f:
                module_map = json.load(f)

            # Parse imports
            parser_instance = VeriASTParser(args.work_dir, module_map)
            imports_graph = parser_instance.parse_imports_from_files()

            # Save imports graph
            imports_path = args.cache_dir / "imports.graph.json"
            args.cache_dir.mkdir(parents=True, exist_ok=True)
            with open(imports_path, "w") as f:
                json.dump(imports_graph, f, indent=2)

            print(f"Parsed {len(imports_graph['edges'])} import edges")
            print(f"Found {len(imports_graph['dynamic_imports'])} dynamic imports")
            print(
                f"Found {len(imports_graph['unresolved_imports'])} unresolved imports"
            )
            print(f"Saved imports graph to {imports_path}")

            return 0

        elif args.command == "run":
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
                workers=args.workers,
                ignore=args.ignore,
                coverage=args.coverage,
                coverage_xml=args.coverage_xml,
                coverage_html=args.coverage_html,
                coverage_source_dirs=args.coverage_source_dirs,
                coverage_omit=args.coverage_omit,
            )

            # Persist per-test results for non-worker mode
            try:
                cache_dir = args.work_dir / ".veri" / "cache"
                cache_dir.mkdir(parents=True, exist_ok=True)
                per_test_path = cache_dir / "last_per_test.json"
                with open(per_test_path, "w", encoding="utf-8") as f:
                    json.dump(getattr(executor, "last_per_test", []), f)
            except Exception as e:  # best-effort
                print(
                    f"Warning: could not write per-test results: {e}", file=sys.stderr
                )

            return exit_code

        elif args.command == "pytest-engine":
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
