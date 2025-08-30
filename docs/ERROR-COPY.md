# veri Error Messages and User Communication

This document provides the authoritative guide for all user-facing error messages, warnings, and informational output in veri. It serves as both a reference for developers and a troubleshooting guide for users.

## Design Principles

### 1. Clarity Over Brevity
- Explain what went wrong and why
- Provide actionable next steps
- Use simple, jargon-free language

### 2. Helpful Context
- Show relevant file paths, line numbers, and code snippets
- Explain the impact of the error
- Link to relevant documentation

### 3. Consistent Format
- Use emoji sparingly but effectively (⚠️ for warnings, ❌ for errors, ✅ for success)
- Include error codes for programmatic handling
- Maintain consistent typography and formatting

### 4. Actionable Guidance
- Always suggest concrete next steps
- Provide command examples when helpful
- Link to relevant documentation sections

## Error Categories

### Collection Errors (E1xxx)

#### E1001: Test Discovery Failed
```
❌ Failed to discover tests

No test files found matching the configured patterns.

Searched in:
  • tests/
  • test_*.py files
  • *_test.py files

To fix this:
  1. Check that test files exist in the expected locations
  2. Verify test file naming follows Python conventions
  3. Update search patterns in veri.toml if needed

For more help: https://docs.veri.dev/troubleshooting#test-discovery
```

#### E1002: Syntax Error in Test File
```
❌ Syntax error in test file

tests/test_parser.py:15:4
    def test_invalid_syntax(
        ^
SyntaxError: unexpected EOF while parsing

veri cannot collect tests from files with syntax errors.

To fix this:
  1. Fix the syntax error in tests/test_parser.py line 15
  2. Run 'python -m py_compile tests/test_parser.py' to verify the fix
  3. Re-run veri

For more help: https://docs.veri.dev/troubleshooting#syntax-errors
```

#### E1003: Import Error During Collection
```
❌ Import error during test collection

Failed to import: tests.test_database
ModuleNotFoundError: No module named 'psycopg2'

This usually means a test dependency is not installed.

To fix this:
  1. Install missing dependencies: pip install psycopg2-binary
  2. Or add to requirements: echo 'psycopg2-binary' >> requirements.txt
  3. Re-run veri

For more help: https://docs.veri.dev/troubleshooting#import-errors
```

### Impact Analysis Errors (E2xxx)

#### E2001: Dynamic Import Detected (Broadening)
```
⚠️ Dynamic import detected - broadening test selection

src/loader.py:42: importlib.import_module(module_name)

Dynamic imports make it difficult to determine which tests are affected by changes.
For safety, veri is running additional tests.

Selected: 156 tests (67% of suite)
Reason: Dynamic import in src.loader -> expanded to package scope

To improve performance:
  1. Consider using static imports where possible
  2. Add type hints to help static analysis
  3. Use --engine pytest for full compatibility

For more help: https://docs.veri.dev/advanced#dynamic-imports
```

#### E2002: Excessive Impact (Broadening) 
```
⚠️ Large impact detected - running all tests

Changes to src/core.py affect 89% of the test suite (214 of 240 tests).
For safety and efficiency, veri is running all tests.

This happens when:
  • Core utility modules are changed
  • Configuration files are modified
  • Import structure is significantly altered

Consider:
  1. Breaking large utility modules into smaller, focused modules
  2. Using dependency injection to reduce coupling
  3. Reviewing import patterns in your codebase

For more help: https://docs.veri.dev/advanced#managing-impact
```

### Configuration Errors (E3xxx)

#### E3001: Invalid Configuration
```
❌ Invalid configuration in veri.toml

Line 5: workers = "invalid"
Expected: integer or "auto", got: "invalid"

Valid examples:
  workers = 4
  workers = "auto"

To fix this:
  1. Edit veri.toml line 5
  2. Use a number (e.g., workers = 4) or "auto"
  3. Re-run veri

For more help: https://docs.veri.dev/configuration
```

#### E3002: Configuration File Not Found
```
⚠️ Configuration file not found

Searched for configuration in:
  • veri.toml
  • pyproject.toml [tool.veri] section

veri will use default settings. To create configuration:

echo 'workers = "auto"' > veri.toml

For more help: https://docs.veri.dev/configuration
```

### Security Errors (E4xxx)

#### E4001: Plugin Not in Allowlist
```
🚨 Blocked plugins detected

These plugins are not in the security allowlist:
  • dangerous-plugin==1.0.0 (HIGH RISK: executes arbitrary code)
  • unknown-plugin==2.1.0 (UNKNOWN: not vetted)

For safety, veri blocked these plugins.

To allow these plugins (if you trust them):
  1. Add to veri.toml:
     [security]
     allowed_plugins = ["dangerous-plugin", "unknown-plugin"]
  
  2. Or disable allowlist (not recommended):
     veri --disable-allowlist

  3. Or use pytest compatibility mode:
     veri --engine pytest

For more help: https://docs.veri.dev/security#plugin-allowlist
```

#### E4002: Network Access Blocked
```
🔒 Network access blocked

A test attempted to make a network connection, but --no-network is enabled.

Failed connection: GET https://api.example.com/data

To fix this:
  1. Use mocks/fixtures instead of real network calls
  2. Remove --no-network flag if network access is needed
  3. Configure test environment to work offline

For more help: https://docs.veri.dev/security#network-isolation
```

### Execution Errors (E5xxx)

#### E5001: Worker Process Crashed
```
❌ Worker process crashed

Worker 2 terminated unexpectedly while running:
  tests/test_memory_intensive.py::test_large_dataset

This usually indicates:
  • Out of memory (test allocating too much RAM)
  • Segmentation fault in C extension
  • Process killed by OS (OOM killer)

To debug:
  1. Run the test individually: veri tests/test_memory_intensive.py::test_large_dataset
  2. Check system memory usage during test
  3. Consider reducing worker count: veri --workers 2

For more help: https://docs.veri.dev/troubleshooting#worker-crashes
```

#### E5002: Test Timeout
```
⏰ Test execution timeout

Test timed out after 300 seconds:
  tests/test_slow_integration.py::test_full_pipeline

Long-running tests can slow down the entire suite.

To fix this:
  1. Optimize the test to run faster
  2. Increase timeout: @pytest.mark.timeout(600)
  3. Mark as slow test: @pytest.mark.slow

For more help: https://docs.veri.dev/configuration#timeouts
```

### CI/Sharding Errors (E6xxx)

#### E6001: Invalid Shard Configuration
```
❌ Invalid shard configuration

Shard index 4 is out of range (valid: 0-3)
Command: veri shard --ci 4

This usually means:
  • Shard manifest has fewer shards than requested
  • Incorrect CI job matrix configuration

To fix this:
  1. Check shard manifest: cat shards.json
  2. Verify CI matrix matches shard count
  3. Re-generate shards if needed: veri split --ci 4

For more help: https://docs.veri.dev/ci#sharding
```

#### E6002: Shard Manifest Not Found
```
❌ Shard manifest not found

Expected: shards.json
Current directory: /workspace/project

Sharding requires a manifest file generated by 'veri split'.

To fix this:
  1. Generate manifest: veri split --ci 4 > shards.json
  2. Or specify path: veri shard --ci 0 --manifest path/to/shards.json
  3. Ensure artifact sharing in CI is working

For more help: https://docs.veri.dev/ci#sharding
```

### Coverage Errors (E7xxx)

#### E7001: Coverage Import Failed
```
❌ Coverage measurement failed

Failed to import coverage library. Coverage tracking is disabled.

Error: ModuleNotFoundError: No module named 'coverage'

To enable coverage:
  1. Install coverage: pip install coverage
  2. Or install with veri: pip install 'veri[coverage]'
  3. Re-run with --cov flag

For more help: https://docs.veri.dev/coverage
```

#### E7002: Coverage Merge Failed
```
❌ Coverage merge failed

Failed to combine coverage data from multiple runs.

Error: Corrupted coverage data in .veri/coverage/worker-2.dat

To fix this:
  1. Clear coverage cache: rm -rf .veri/coverage/
  2. Re-run tests with --cov
  3. Check disk space and permissions

For more help: https://docs.veri.dev/troubleshooting#coverage-issues
```

## Warning Messages

### Performance Warnings

#### W1001: Large Test Suite
```
⚠️ Large test suite detected (12,450 tests)

Large test suites may experience slower startup times on first run.
Consider using impact-aware execution (default) instead of --all.

Performance tips:
  • Use veri (default) instead of veri --all for daily development
  • Enable watch mode: veri -w for continuous development
  • Consider test suite organization and cleanup

For more help: https://docs.veri.dev/performance#large-suites
```

#### W1002: Cache Miss
```
⚠️ Cache invalidated - rebuilding test index

Detected changes in:
  • Python version (3.10 -> 3.11)
  • Installed packages (requirements.txt modified)

First run after cache invalidation may be slower than usual.
Subsequent runs will be fast.

Cache location: .veri/cache/
```

### Compatibility Warnings

#### W2001: Plugin Compatibility
```
⚠️ Plugin compatibility warning

pytest-custom-plugin may not work optimally with veri.

Known issues:
  • Custom collection hooks may be ignored
  • May interfere with impact analysis

Recommendations:
  • Test thoroughly in your environment
  • Use --engine pytest if needed for full compatibility
  • Report issues: https://github.com/dennis8/veri/issues

For more help: https://docs.veri.dev/compatibility
```

#### W2002: Deprecated Feature
```
⚠️ Deprecated feature usage

The --legacy-mode flag is deprecated and will be removed in v2.0.

Please migrate to:
  • Use standard pytest-compatible syntax
  • Update configuration to modern format
  • See migration guide: https://docs.veri.dev/migration

For more help: https://docs.veri.dev/deprecations
```

## Informational Messages

### Success Messages

#### I1001: Impact Analysis Summary
```
⚡ veri completed successfully

Tests: 23 selected, 23 passed (0.8s)
Impact: 3.2% of suite (changes to src/utils.py)
Cache: Hit (warm)

Why these tests ran:
  src/utils.py → tests/test_utils.py (4 tests)
  src/utils.py → src/parser.py → tests/test_parser.py (19 tests)

Use --explain for detailed analysis.
```

#### I1002: Watch Mode Ready  
```
⚡ veri watch mode active

Watching for changes in:
  • src/ (Python files)
  • tests/ (test files)
  • conftest.py files

Press Ctrl+C to stop. Edit any file to trigger test run.

Pro tip: Use 'veri --explain' to understand test selection.
```

### Status Messages

#### I2001: First Run Setup
```
ℹ️ First run detected - building caches

This will take longer than usual as veri:
  1. Collects all tests and builds index ✓
  2. Analyzes import relationships ✓  
  3. Creates dependency graph (in progress...)
  4. Caches results for future runs

Subsequent runs will be much faster (~80% time reduction).
```

#### I2002: Telemetry Status
```
📊 Telemetry Status

Current settings:
  • Enabled: ❌ No (respecting DO_NOT_TRACK)
  • Session ID: Not applicable
  • Data collection: None

To enable anonymous usage analytics:
  1. Unset DO_NOT_TRACK environment variable
  2. Add 'telemetry_enabled = true' to veri.toml

Learn more: https://docs.veri.dev/telemetry
```

## Debug Output (Verbose Mode)

### Collection Phase
```
[DEBUG] Collection phase starting
[DEBUG] Scanning test directories: ['tests/', 'src/tests/']
[DEBUG] Found test files: 45 files, 234 test cases
[DEBUG] Collection completed in 0.15s

[DEBUG] Import analysis starting
[DEBUG] Parsing AST for 67 Python files
[DEBUG] Built import graph: 156 modules, 342 edges
[DEBUG] Computed reverse dependencies in 0.02s
[DEBUG] Import analysis completed in 0.18s
```

### Impact Analysis
```
[DEBUG] Impact analysis starting
[DEBUG] Changed files detected: ['src/parser.py']
[DEBUG] Affected modules: ['src.parser', 'src.ast_utils', 'tests.test_parser']
[DEBUG] Transitive closure: 3 modules → 12 test files → 23 test cases
[DEBUG] Impact ratio: 9.8% (below 60% threshold)
[DEBUG] Impact analysis completed in 0.03s
```

### Execution Phase
```
[DEBUG] Execution phase starting
[DEBUG] Worker pool: 4 processes (PID: 1234, 1235, 1236, 1237)
[DEBUG] Scheduling 23 tests across 4 workers
[DEBUG] Using historical timings for 19/23 tests
[DEBUG] Load balancing: W1=6.2s, W2=6.1s, W3=6.0s, W4=5.9s
[DEBUG] Execution completed in 6.2s (95% efficiency)
```

## Exit Code Reference

| Code | Meaning | Message |
|------|---------|---------|
| 0 | Success | All tests passed |
| 1 | Test failures | One or more tests failed |
| 2 | Collection error | Failed to collect tests (syntax errors, import issues) |
| 3 | Internal error | Unexpected veri error (please report) |
| 4 | Usage error | Invalid command line arguments |
| 5 | No tests found | No tests matched selection criteria |

## Message Formatting Guidelines

### Error Messages
```
❌ [COMPONENT] Error description

Context and details about what went wrong.

Specific error information:
  • File: path/to/file.py
  • Line: 42
  • Details: specific error details

To fix this:
  1. First step
  2. Second step
  3. Alternative approach

For more help: https://docs.veri.dev/section
```

### Warning Messages  
```
⚠️ Warning description

Brief explanation of the issue and its impact.

Consider:
  • Suggestion 1
  • Suggestion 2

For more help: https://docs.veri.dev/section
```

### Informational Messages
```
ℹ️ Information title

Relevant details about the current operation or status.

Additional context if helpful.
```

## Localization Considerations

While veri v1.0 supports English only, the message system is designed for future localization:

- Message keys for programmatic access
- Separated format strings and data
- Consistent terminology and phrasing
- Cultural considerations for examples and references

## Testing Error Messages

All error messages should be tested for:

- **Accuracy**: Message matches the actual error condition
- **Clarity**: Users can understand what went wrong
- **Actionability**: Suggested fixes actually resolve the issue
- **Formatting**: Consistent with style guidelines
- **Links**: Documentation links are valid and helpful

Example test:
```python
def test_plugin_allowlist_error_message():
    result = run_veri(['--plugin', 'dangerous-plugin'])
    assert result.returncode == 4
    assert '🚨 Blocked plugins detected' in result.stderr
    assert 'dangerous-plugin' in result.stderr
    assert 'allowed_plugins' in result.stderr
    assert 'https://docs.veri.dev/security' in result.stderr
```

This comprehensive error message guide ensures consistent, helpful communication with veri users across all scenarios.