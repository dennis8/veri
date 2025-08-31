# veri CLI Specification

This document provides the complete specification for veri's command-line interface, configuration options, and exit codes.

## Command Line Interface

### Basic Usage

```bash
veri [OPTIONS] [TEST_PATHS...]
```

### Global Options

#### Test Selection
- `-a, --all` - Run all tests (bypass impact analysis)
- `-k EXPRESSION` - Run tests matching the given expression
- `-m MARKERS` - Run tests matching given markers
- `--last-failed` - Run only tests that failed in the last run
- `--engine pytest` - Hand off execution to pytest (escape hatch)

#### Execution Control  
- `--workers N` - Number of parallel workers (default: auto)
- `--workers auto` - Automatically detect optimal worker count
- `-x, --maxfail N` - Stop after N failures (default: unlimited)
- `--no-capture` - Don't capture stdout/stderr (equivalent to pytest -s)

#### Watch Mode
- `-w, --watch` - Run in watch mode, re-running tests on file changes
- `--watch-ignore PATTERN` - Ignore files matching pattern in watch mode

#### Output & Reporting
- `-q, --quiet` - Decrease verbosity
- `-v, --verbose` - Increase verbosity  
- `-vv` - Extra verbose output
- `--no-color` - Disable colored output
- `--junit-xml PATH` - Generate JUnit XML report
- `--jsonl PATH` - Generate JSONL event stream

#### Coverage
- `--cov [PATHS]` - Enable coverage measurement
- `--cov-report TYPE` - Coverage report type (xml, html, term)
- `--cov-merge-full` - Generate full coverage report after selective run
- `--cov-diff-threshold N` - Fail if changed lines coverage below N%

#### CI/Sharding
- `split --ci N` - Generate sharding manifest for N shards
- `shard --ci I` - Run shard I from manifest  
- `--manifest PATH` - Path to sharding manifest file

#### Diagnostics
- `--explain` - Show detailed reasoning for test selection
- `--version` - Show version information
- `--telemetry-status` - Show telemetry and security status

#### Security
- `--no-network` - Block network access during test execution
- `--disable-allowlist` - Disable plugin allowlist enforcement (with warning)

### Subcommands

#### `veri split`
Generate a sharding manifest for CI parallelization.

```bash
veri split --ci N [OPTIONS]
```

**Options:**
- `--ci N` - Number of shards to create (required)
- `--output PATH` - Output manifest file (default: stdout)
- `--strategy NAME` - Sharding strategy: `timings`, `round-robin` (default: timings)

**Example:**
```bash
veri split --ci 4 --output shards.json
```

#### `veri shard`  
Execute a specific shard from a manifest.

```bash
veri shard --ci I [OPTIONS]
```

**Options:**
- `--ci I` - Shard index to execute (0-based, required)
- `--manifest PATH` - Path to sharding manifest (default: shards.json)

**Example:**
```bash
veri shard --ci 0 --manifest shards.json --junit-xml reports/junit-0.xml
```

## Configuration

veri supports configuration via multiple sources with the following precedence:

1. Command line flags (highest priority)
2. Environment variables
3. Configuration files
4. Default values (lowest priority)

### Configuration Files

#### pyproject.toml
```toml
[tool.veri]
# Basic options
workers = "auto"
coverage = true
max_failures = 1

# Watch mode
watch = false
watch_ignore = ["*.log", "tmp/", "__pycache__/"]

# Output
quiet = false
no_color = false
junit_xml = "reports/junit.xml"

# Coverage
cov_report = ["xml", "html"]
cov_diff_threshold = 80

# CI options  
[tool.veri.ci]
shards = 4
strategy = "timings"

# Security
[tool.veri.security]
enforce_allowlist = true
allowed_plugins = ["custom-plugin>=1.0"]
no_network = false

# Telemetry
[tool.veri.telemetry]
enabled = false
```

#### veri.toml (alternative)
```toml
workers = 8
coverage = true
max_failures = 1

[ci]
shards = 4
strategy = "timings"

[security]
enforce_allowlist = true
allowed_plugins = ["custom-plugin>=1.0"]

[telemetry]
enabled = false
```

### Environment Variables

All configuration options can be set via environment variables with the `VERI_` prefix:

```bash
# Basic execution
export VERI_WORKERS=4
export VERI_MAX_FAILURES=1
export VERI_COVERAGE=true

# Output control
export VERI_QUIET=true
export VERI_NO_COLOR=true
export VERI_JUNIT_XML=reports/junit.xml

# Security
export VERI_NO_NETWORK=true
export VERI_DISABLE_ALLOWLIST=true

# Telemetry opt-out (multiple standards supported)
export DO_NOT_TRACK=1
export VERI_NO_TELEMETRY=1
export NO_ANALYTICS=1

# Cache control
export VERI_CACHE_DIR=/tmp/veri-cache
export VERI_CLEAR_CACHE=true
```

### Configuration Discovery

veri searches for configuration files in the following order:

1. `veri.toml` in current directory
2. `pyproject.toml` with `[tool.veri]` section in current directory  
3. Walk up directory tree looking for the above files
4. Global config in `~/.config/veri/config.toml` (if exists)

## Exit Codes

veri uses specific exit codes to communicate results:

| Code | Meaning | Description |
|------|---------|-------------|
| 0 | Success | All tests passed |
| 1 | Test failures | One or more tests failed |
| 2 | Test collection error | Failed to collect or parse tests |
| 3 | Internal error | Unexpected veri error (bugs) |
| 4 | Usage error | Invalid command line arguments |
| 5 | No tests found | No tests matched selection criteria |

### Exit Code Examples

```bash
# Success - all tests pass
veri
echo $?  # 0

# Test failures
veri --maxfail=1
echo $?  # 1 (some tests failed)

# Collection error (syntax error in test file)
veri tests/broken_syntax.py  
echo $?  # 2

# Usage error
veri --invalid-flag
echo $?  # 4

# No tests found
veri -k "nonexistent_pattern"
echo $?  # 5
```

## Output Formats

### Standard Output

#### Default Format
```
⚡ veri v0.1.0 - Ultra-fast, pytest-compatible test runner

Collecting tests... ✓ (234 found in 0.1s)
Planning... ✓ (12 impacted by changes to src/parser.py)

tests/test_parser.py::test_basic PASSED  [ 25%]
tests/test_parser.py::test_edge  FAILED  [ 50%]
tests/test_ast.py::test_parse    PASSED  [ 75%]
tests/integration/test_full.py  PASSED  [100%]

====== FAILURES ======
tests/test_parser.py::test_edge - AssertionError: Expected 'foo', got 'bar'

====== 3 passed, 1 failed in 0.8s ======
```

#### Quiet Mode (`-q`)
```
F...
====== FAILURES ======
tests/test_parser.py::test_edge - AssertionError: Expected 'foo', got 'bar'
====== 3 passed, 1 failed in 0.8s ======
```

#### Verbose Mode (`-vv`)  
```
⚡ veri v0.1.0 - Ultra-fast, pytest-compatible test runner

Configuration:
  Workers: 4 (auto-detected)
  Coverage: enabled  
  Cache: .veri/cache (1.2MB, 234 tests indexed)

Collection phase:
  Scanning 45 Python files... ✓ (0.05s)
  Found 234 test cases in 23 files ✓ (0.08s)
  
Impact analysis:
  Changed files: src/parser.py
  Reverse dependencies: src.ast, tests.test_parser, tests.integration.test_full
  Selected: 12 tests (5.1% of total) ✓ (0.02s)

Execution phase:
  Worker pool: 4 processes ready ✓ (0.15s)
  
tests/test_parser.py::test_basic PASSED                    [W1] 0.02s
tests/test_parser.py::test_edge FAILED                     [W2] 0.15s  
tests/test_ast.py::test_parse PASSED                       [W3] 0.05s
tests/integration/test_full.py::test_pipeline PASSED      [W1] 0.32s

====== FAILURES ======
tests/test_parser.py::test_edge - AssertionError: Expected 'foo', got 'bar'
  File "tests/test_parser.py", line 15, in test_edge
    assert result == 'foo'

Performance:
  Collection: 0.15s (2.3x faster than pytest)
  Execution: 0.54s (4 workers, 87% efficiency)
  Total: 0.69s (6.2x faster than full run)

====== 3 passed, 1 failed in 0.8s ======
```

### JUnit XML Output

When `--junit-xml` is specified, veri generates standard JUnit XML:

```xml
<?xml version="1.0" encoding="utf-8"?>
<testsuites>
  <testsuite name="veri" tests="4" failures="1" skipped="0" errors="0" time="0.54">
    <testcase classname="tests.test_parser" name="test_basic" time="0.02"/>
    <testcase classname="tests.test_parser" name="test_edge" time="0.15">
      <failure message="AssertionError: Expected 'foo', got 'bar'">
        File "tests/test_parser.py", line 15, in test_edge
          assert result == 'foo'
      </failure>
    </testcase>
    <testcase classname="tests.test_ast" name="test_parse" time="0.05"/>
    <testcase classname="tests.integration.test_full" name="test_pipeline" time="0.32"/>
  </testsuite>
</testsuites>
```


### Parallel Execution & Worker Pool

- Flag: `--workers N` (N>=1) enables multi‑worker execution.
- Scheduling strategy defaults to Balanced and uses historical timings when available.
- Worker pool reliability:
  - Startup handshake with `HelloOk` and `startup_timeout_sec` guard.
  - Heartbeats (`HealthCheck`/`HealthOk`) every `heartbeat_interval_sec`.
  - Per‑batch `execution_timeout_sec` with automatic worker restart.
- Coverage:
  - Per‑worker coverage data (`.coverage.worker_<id>`) combined automatically.
  - `--cov-merge-full` writes XML/JSON/HTML reports to `reports/`.

Config (veri.toml / [tool.veri]):

```
[worker]
startup_timeout_sec = 30
execution_timeout_sec = 300
heartbeat_interval_sec = 10
```

### JSONL Event Stream

When `--jsonl` is specified, veri outputs newline-delimited JSON events:

```json
{"t":"start","timestamp":"2025-08-30T10:41:03Z","engine":"veri","version":"0.1.0","workers":4}
{"t":"collection","files_scanned":45,"tests_found":234,"duration_ms":150}
{"t":"plan","changed_files":["src/parser.py"],"impacted_tests":12,"total_tests":234,"duration_ms":20}
{"t":"case","nodeid":"tests/test_parser.py::test_basic","status":"pass","duration_ms":18,"worker":1}
{"t":"case","nodeid":"tests/test_parser.py::test_edge","status":"fail","duration_ms":154,"worker":2,"error":"AssertionError: Expected 'foo', got 'bar'"}
{"t":"case","nodeid":"tests/test_ast.py::test_parse","status":"pass","duration_ms":47,"worker":3}
{"t":"case","nodeid":"tests/integration/test_full.py::test_pipeline","status":"pass","duration_ms":321,"worker":1}
{"t":"summary","passed":3,"failed":1,"skipped":0,"errors":0,"total_duration_ms":540}
```

## Advanced Usage Examples

### Watch Mode with Coverage
```bash
# Development workflow
veri -w --cov --cov-report html
```

### CI Sharding with Coverage
```bash
# Generate shards (run once)
veri split --ci 4 > shards.json

# Run in parallel (each CI job)
veri shard --ci $CI_JOB_INDEX --junit-xml reports/junit-$CI_JOB_INDEX.xml --cov

# Combine coverage (final step)
veri --cov-merge-full --cov-report xml
```

### Debugging Failed Tests
```bash
# Run only failed tests with maximum verbosity
veri --last-failed -vv --no-capture

# Explain why specific tests were selected
veri --explain -k "test_parser"
```

### Security-conscious Execution
```bash
# Maximum security for production CI
veri --no-network --cov --junit-xml reports/junit.xml
```

### Escape Hatch for Compatibility
```bash
# Use pytest engine for specific plugins
veri --engine pytest --plugin special-plugin
```

## Integration with Development Tools

### VS Code Tasks
```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "veri: run tests",
      "type": "shell", 
      "command": "veri",
      "group": "test",
      "presentation": {
        "echo": true,
        "reveal": "always",
        "panel": "new"
      }
    },
    {
      "label": "veri: watch mode",
      "type": "shell",
      "command": "veri -w",
      "group": "test",
      "isBackground": true
    }
  ]
}
```

### Makefile Integration
```makefile
.PHONY: test test-watch test-all coverage

test:
	veri

test-watch:
	veri -w

test-all:
	veri -a

coverage:
	veri --cov --cov-report html
```

### GitHub Actions Integration
```yaml
- name: Setup Python & Install Dependencies  
  uses: ./.github/actions/setup-python
  
- name: Install veri
  run: uv tool install veri
  
- name: Run tests with coverage
  run: veri --cov --junit-xml reports/junit.xml --jsonl reports/events.jsonl
  
- name: Upload test results
  uses: actions/upload-artifact@v4
  if: always()
  with:
    name: test-results
    path: reports/
```

This specification covers the complete CLI interface, configuration system, and integration patterns for veri. For more detailed examples and migration guides, see [MIGRATION.md](MIGRATION.md).
### Execution Model

veri always executes tests through a Python worker pool. The pool supports `workers = 1` (single-process) and `workers > 1` (parallel), but orchestration and reporting are unified:

- The scheduler creates batches; the pool executes them via a JSONL protocol; the CLI aggregates and prints a unified summary.
- Workers are launched via `uv run --project py_worker` and execute a cached worker script with absolute paths for consistent imports.
- The final line reports per‑outcome counts for all runs: `Summary: <passed> passed, <skipped> skipped, <failed> failed, <error> error (<total> total)`.

If plugin incompatibilities are detected and allowlisting is enforced, veri can fall back to the pytest engine via `--engine pytest`.
