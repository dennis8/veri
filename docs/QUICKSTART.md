# Quick Start Guide

Get up and running with veri in under 5 minutes. This guide covers installation, basic usage, and common workflows.

## Prerequisites

- Python 3.9+ 
- Existing pytest-based test suite (or create a simple one)
- Git repository (optional, but recommended for impact analysis)

## Installation

### Option 1: Using uv (Recommended)
```bash
# Install uv if you haven't already
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install veri
uv tool install veri

# Verify installation
veri --version
```

### Option 2: Using pip
```bash
pip install veri

# Verify installation  
veri --version
```

### Option 3: Using pipx
```bash
# Install pipx if you haven't already
python -m pip install pipx

# Install veri
pipx install veri

# Verify installation
veri --version
```

## First Run

Navigate to your Python project with tests and run:

```bash
# First run - builds cache (may take a moment)
veri -a

# Expected output:
⚡ veri v0.1.0 - Ultra-fast, pytest-compatible test runner

Collecting tests... ✓ (127 found in 0.2s)
Building cache... ✓ (import graph, timings)

tests/test_utils.py::test_parse PASSED     [ 23%]
tests/test_api.py::test_create PASSED      [ 46%]  
tests/test_models.py::test_save PASSED     [ 69%]
...

====== 127 passed in 8.4s ======
```

🎉 **Congratulations!** veri is now working with your test suite.

## Basic Usage

### Daily Development Workflow

```bash
# Run only tests affected by your changes (default)
veri

# Watch mode - automatically re-run tests when files change
veri -w

# Run specific tests
veri tests/test_api.py
veri -k "test_user_creation"

# Run tests with coverage
veri --cov
```

### Understanding Test Selection

```bash
# See why specific tests were selected
veri --explain

# Example output:
Changed files (2):
  src/models.py
  src/utils.py

Impacted tests (23):
  tests/test_models.py::test_user_create (direct import)
  tests/test_api.py::test_signup (via src.models -> tests.test_api)
  tests/test_utils.py::test_validate (direct import)
  ...

Safety notes: No dynamic imports detected
```

## Common Workflows

### Development Loop
```bash
# Start watch mode in a terminal
veri -w

# In your editor:
# 1. Edit src/models.py
# 2. Save file
# 3. veri automatically runs affected tests
# 4. Fix any failures
# 5. Repeat
```

### Pre-commit Checks
```bash
# Run all tests with coverage
veri -a --cov

# Or just run affected tests
veri --cov
```

### CI Integration

#### Simple GitHub Actions
```yaml
name: Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v4
        with:
          python-version: '3.11'
      
      - name: Install dependencies
        run: |
          pip install uv
          uv tool install veri
          uv sync  # or: pip install -r requirements.txt
      
      - name: Run tests
        run: veri --cov --junit-xml reports/junit.xml
```

#### Parallel CI with Sharding
```yaml
strategy:
  matrix:
    shard: [0, 1, 2, 3]
steps:
  - name: Generate shards
    if: matrix.shard == 0
    run: veri split --ci 4 > shards.json
    
  - name: Run shard
    run: veri shard --ci ${{ matrix.shard }} --junit-xml junit-${{ matrix.shard }}.xml
```

## Configuration

### Zero Configuration
veri works out of the box with sensible defaults. No configuration needed for most projects.

### Optional Configuration

Create `veri.toml` in your project root:

```toml
# Basic settings
workers = "auto"  # or specific number: workers = 4
coverage = true

# Watch mode settings
watch_ignore = ["*.log", "tmp/", "__pycache__/"]

# CI settings
[ci]
shards = 4
junit_xml = "reports/junit.xml"
```

Or add to your existing `pyproject.toml`:

```toml
[tool.veri]
workers = "auto"
coverage = true

[tool.veri.ci]
shards = 4
```

## Migrating from pytest

veri is a drop-in replacement for most pytest workflows:

| pytest | veri | Notes |
|--------|------|-------|
| `pytest` | `veri -a` | Full run (first time) |
| `pytest` | `veri` | Impact-aware (subsequent) |
| `pytest -n auto` | `veri --workers auto` | Parallel execution |
| `pytest --cov` | `veri --cov` | Coverage |
| `pytest -k "pattern"` | `veri -k "pattern"` | Test selection |
| `pytest -m "slow"` | `veri -m "slow"` | Marker filtering |

### Plugin Compatibility

Most pytest plugins work with veri:

✅ **Works out of the box:**
- pytest-cov (replaced by faster veri coverage)
- pytest-mock
- pytest-asyncio
- pytest-django
- pytest-flask

⚠️ **May need adjustments:**
- pytest-xdist (replaced by veri's built-in parallelization)
- pytest-split (replaced by veri's sharding)

❌ **Use escape hatch if needed:**
```bash
# Fall back to pytest for problematic plugins
veri --engine pytest --plugin special-plugin
```

## Performance Expectations

Typical improvements over pytest:

- **First run**: 2-3x faster collection
- **Impact-aware runs**: 5-20x faster (depending on change size)
- **Watch mode**: Sub-second feedback vs 5-30s
- **CI with sharding**: 20-40% faster overall

### Example Performance

Before (pytest):
```bash
$ time pytest -n auto
# 127 tests, 45 seconds
```

After (veri):
```bash
$ time veri -a  # First run
# 127 tests, 18 seconds (2.5x faster)

$ echo "# comment" >> src/utils.py
$ time veri     # Impact-aware
# 12 tests, 2.1 seconds (21x faster)
```

## Troubleshooting

### Tests Not Found
```bash
# Check test discovery
veri --collect-only

# If empty, check file patterns:
ls tests/test_*.py  # Standard naming
ls *_test.py        # Alternative naming
```

### Performance Not Improved
```bash
# Ensure you're using impact analysis (not -a flag)
veri  # Good - uses impact analysis
veri -a  # Slower - runs all tests

# Check impact ratio
veri --explain  # Should show < 100% of tests
```

### Plugin Issues
```bash
# Check which plugins are loaded
veri --explain

# Allow additional plugins
echo 'allowed_plugins = ["your-plugin"]' >> veri.toml

# Or use pytest engine temporarily
veri --engine pytest
```

### Coverage Issues
```bash
# Install coverage if missing
pip install coverage

# Clear cache if corrupt
rm -rf .veri/cache/
veri -a --cov
```

## Getting Help

### Documentation
- **Full docs**: [https://docs.veri.dev](https://docs.veri.dev)
- **CLI reference**: `veri --help`
- **Configuration**: [SPEC.md](SPEC.md)
- **Migration guide**: [MIGRATION.md](MIGRATION.md)

### Common Commands
```bash
# Show version and build info
veri --version

# Explain test selection
veri --explain

# Check security and telemetry status
veri --telemetry-status

# Run in verbose mode for debugging
veri -vv
```

### Community Support
- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Questions and community help
- **Discord**: Real-time community chat (link in GitHub README)

## Next Steps

Once you're comfortable with basic usage:

1. **Set up CI integration** - Use veri's sharding for faster CI
2. **Configure coverage** - Enable diff coverage for PR workflows
3. **Explore watch mode** - Integrate into your daily development
4. **Optimize configuration** - Tune workers and ignore patterns
5. **Share with team** - Help teammates migrate from pytest

## Advanced Examples

### Complex CI Workflow
```bash
# Multi-platform, multi-shard CI
veri split --ci 4 > shards.json
veri shard --ci $SHARD --cov --junit-xml junit-$SHARD.xml
veri --cov-merge-full --cov-report xml  # Final step
```

### Development with Coverage Gate
```bash
# Run with diff coverage threshold
veri --cov --cov-diff-threshold 80
```

### Security-conscious Execution
```bash
# Block network access and enforce plugin allowlist
veri --no-network --cov
```

### Custom Configuration
```toml
# Advanced veri.toml
workers = 8
coverage = true
cov_diff_threshold = 85

[security]
enforce_allowlist = true
allowed_plugins = ["company-internal-plugin"]

[ci]
shards = 6
strategy = "timings"
```

That's it! You're now ready to use veri effectively. The tool is designed to be intuitive, so explore and experiment. Most pytest knowledge transfers directly to veri.

## Parallel Execution

Use `--workers N` to run tests in parallel. Veri schedules tests across workers to minimize wall‑clock time and prioritizes short or likely‑to‑fail tests depending on strategy.

Config (in veri.toml or [tool.veri] in pyproject.toml):

```
[worker]
# Worker process startup timeout (seconds)
startup_timeout_sec = 30
# Per‑batch execution timeout (seconds)
execution_timeout_sec = 300
# Heartbeat ping interval (seconds)
heartbeat_interval_sec = 10
```

Notes:
- Veri launches Python workers using the project’s py_worker environment (via `uv run`) so pytest and plugins resolve consistently.
- Coverage in multi‑worker mode writes per‑worker data files and is combined automatically; `--cov-merge-full` emits XML/JSON/HTML.

See also: `docs/TROUBLESHOOTING.md` for common issues and fixes.
