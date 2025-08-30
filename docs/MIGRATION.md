# Migration Guide: From pytest to veri

This guide helps you migrate from pytest-based test workflows to veri, whether you're working on a personal project or migrating a large team.

## Quick Migration Checklist

- [ ] Install veri: `uv tool install veri`
- [ ] Replace `pytest` with `veri` in your commands
- [ ] Update CI configuration to use veri sharding
- [ ] Review plugin compatibility and update if needed
- [ ] Configure any custom settings in `veri.toml` or `pyproject.toml`
- [ ] Test the migration with a subset of your test suite

## Basic Command Mapping

### Individual Developer Workflow

| pytest Command | veri Equivalent | Notes |
|----------------|-----------------|--------|
| `pytest` | `veri -a` | Full test run (first time to build cache) |
| `pytest` | `veri` | Impact-aware testing (subsequent runs) |
| `pytest -k "pattern"` | `veri -k "pattern"` | Same syntax |
| `pytest -m "marker"` | `veri -m "marker"` | Same syntax |
| `pytest --lf` | `veri --last-failed` | Same functionality |
| `pytest -x` | `veri -x` | Stop on first failure |
| `pytest -v` | `veri -v` | Verbose output |
| `pytest -s` | `veri --no-capture` | Don't capture output |

### Parallel Execution

| pytest Command | veri Equivalent | Performance Gain |
|----------------|-----------------|------------------|
| `pytest -n auto` | `veri --workers auto` | 2-5x faster collection |
| `pytest -n 4` | `veri --workers 4` | Single collection vs per-worker |
| `pytest -n auto --dist worksteal` | `veri --workers auto` | Built-in intelligent scheduling |

### Coverage

| pytest Command | veri Equivalent | Performance Gain |
|----------------|-----------------|------------------|
| `pytest --cov` | `veri --cov` | Incremental coverage |
| `pytest --cov --cov-report html` | `veri --cov --cov-report html` | Same output format |
| `coverage combine` | `veri --cov-merge-full` | 10-50x faster combining |

## CI/CD Migration

### Before: Multiple Tools
```yaml
# Complex pytest + plugin setup
- name: Install dependencies
  run: |
    pip install pytest pytest-xdist pytest-cov pytest-split
    
- name: Run tests  
  run: |
    pytest --cov --cov-report xml -n auto
    coverage combine
    
- name: Sharded CI (separate action)
  run: |
    pytest-split --splits 4 --group $GROUP_ID
```

### After: Single Tool
```yaml
# Simple veri setup
- name: Install veri
  run: uv tool install veri
  
- name: Run tests
  run: veri --cov --junit-xml reports/junit.xml
  
# For sharded CI:
- name: Run shard
  run: |
    veri split --ci 4 > shards.json  # Run once
    veri shard --ci $SHARD_ID --cov --junit-xml reports/junit-$SHARD_ID.xml
```

## Platform-Specific Migration

### GitHub Actions

**Before:**
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
      - run: pip install -r requirements.txt
      - run: pip install pytest pytest-xdist pytest-cov
      - run: pytest -n auto --cov --junitxml=reports/junit.xml
      - run: coverage xml
```

**After:**
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
      - run: pip install uv && uv tool install veri
      - run: uv sync  # or pip install -r requirements.txt
      - run: veri --cov --junit-xml reports/junit.xml
```

### GitLab CI

**Before:**
```yaml
test:
  script:
    - pip install pytest pytest-xdist pytest-cov
    - pytest -n auto --cov --junitxml=reports/junit.xml
    - coverage xml
  artifacts:
    reports:
      junit: reports/junit.xml
      coverage_report:
        coverage_format: cobertura
        path: coverage.xml
```

**After:**
```yaml
test:
  script:
    - pip install uv && uv tool install veri
    - veri --cov --junit-xml reports/junit.xml
  artifacts:
    reports:
      junit: reports/junit.xml
      coverage_report:
        coverage_format: cobertura  
        path: coverage.xml
```

### Azure Pipelines

**Before:**
```yaml
- task: UsePythonVersion@0
  inputs:
    versionSpec: '3.11'
- script: |
    pip install pytest pytest-xdist pytest-cov
    pytest -n auto --cov --junitxml=reports/junit.xml
    coverage xml
- task: PublishTestResults@2
  inputs:
    testResultsFiles: 'reports/junit.xml'
```

**After:**
```yaml
- task: UsePythonVersion@0
  inputs:
    versionSpec: '3.11'
- script: |
    pip install uv && uv tool install veri
    veri --cov --junit-xml reports/junit.xml
- task: PublishTestResults@2
  inputs:
    testResultsFiles: 'reports/junit.xml'
```

## Multi-Shard CI Migration

### Before: Complex Plugin Setup
```yaml
strategy:
  matrix:
    group: [1, 2, 3, 4]
steps:
  - run: pip install pytest pytest-split
  - run: pytest $(pytest-split --splits=4 --group=${{ matrix.group }})
```

### After: Built-in Sharding
```yaml
strategy:
  matrix:
    shard: [0, 1, 2, 3]
steps:
  - run: uv tool install veri
  - run: veri split --ci 4 > shards.json  # Only in shard 0
  - run: veri shard --ci ${{ matrix.shard }} --junit-xml junit-${{ matrix.shard }}.xml
```

## Configuration Migration

### pytest.ini → veri.toml

**Before (pytest.ini):**
```ini
[tool:pytest]
minversion = 6.0
addopts = -ra -q --cov --cov-report html
testpaths = tests
markers =
    slow: marks tests as slow
    integration: marks tests as integration tests
```

**After (pyproject.toml):**
```toml
[tool.veri]
coverage = true
cov_report = ["html"]
quiet = true

# Same markers work automatically
```

### tox.ini Integration

**Before:**
```ini
[testenv]
deps = pytest pytest-xdist pytest-cov
commands = pytest -n auto --cov {posargs}
```

**After:**
```ini
[testenv]
deps = veri
commands = veri --cov {posargs}
```

## Plugin Compatibility

### Supported Plugins (Drop-in Replacement)

✅ **Core pytest features:**
- Fixtures and dependency injection
- Parametrized tests (`@pytest.mark.parametrize`)
- Markers and mark expressions
- Test discovery and collection

✅ **Popular plugins that work out of the box:**
- `pytest-asyncio` - Async test support
- `pytest-mock` - Mocking utilities  
- `pytest-django` - Django integration
- `pytest-flask` - Flask integration
- `pytest-cov` - Coverage (replaced by veri's faster implementation)

✅ **Framework integrations:**
- FastAPI test client
- aiohttp test utilities
- SQLAlchemy testing patterns

### Plugins Replaced by veri

❌ **No longer needed:**
- `pytest-xdist` → Use `veri --workers`
- `pytest-split` → Use `veri split/shard --ci`
- `pytest-testmon` → Built-in impact analysis
- `pytest-watcher` → Use `veri -w`
- `pytest-benchmark` → Use `veri` performance tracking

### Plugins Requiring Fallback

⚠️ **May need `--engine pytest`:**
- Complex collection-mutating plugins
- Plugins that heavily modify pytest internals
- Plugins not in the default allowlist

**How to handle:**
```bash
# Test with specific plugin
veri --disable-allowlist --plugin special-plugin

# Or use escape hatch
veri --engine pytest --plugin special-plugin
```

## Team Migration Strategy

### Phase 1: Developer Adoption (Week 1)
1. **Install veri locally:**
   ```bash
   uv tool install veri
   ```

2. **Start with impact-aware testing:**
   ```bash
   # Replace daily pytest usage
   veri -w  # Watch mode for development
   ```

3. **Verify compatibility:**
   ```bash
   # Run full suite to ensure compatibility
   veri -a
   ```

### Phase 2: CI Integration (Week 2)
1. **Update CI to use veri:**
   - Start with non-critical branches
   - Compare performance with existing setup
   - Validate test results match pytest exactly

2. **Migrate CI artifacts:**
   - Update JUnit XML paths if needed
   - Verify coverage reports work with existing tools

### Phase 3: Advanced Features (Week 3+)
1. **Optimize CI with sharding:**
   ```bash
   veri split --ci N  # Replace pytest-split
   ```

2. **Enable coverage optimization:**
   ```bash
   veri --cov --cov-merge-full  # Replace coverage combine
   ```

3. **Configure project-specific settings:**
   ```toml
   [tool.veri]
   workers = 8
   cov_diff_threshold = 80
   ```

## Troubleshooting Common Issues

### Test Collection Differences

**Issue:** Tests found by pytest but not veri
**Solution:** 
```bash
# Compare collection
pytest --collect-only > pytest_tests.txt
veri -a --collect-only > veri_tests.txt
diff pytest_tests.txt veri_tests.txt
```

### Plugin Conflicts

**Issue:** Plugin not working with veri
**Solution:**
```bash
# Check plugin status
veri --explain

# Try with allowlist disabled
veri --disable-allowlist

# Use escape hatch if needed
veri --engine pytest
```

### Performance Not as Expected

**Issue:** veri not faster than pytest
**Solution:**
```bash
# Ensure cache is built
veri -a  # First run builds cache

# Check impact analysis
veri --explain  # Should show < 100% of tests

# Verify parallel execution
veri --workers auto -v  # Should show multiple workers
```

### Coverage Report Differences

**Issue:** Coverage reports don't match
**Solution:**
```bash
# Force full coverage report
veri --cov --cov-merge-full

# Compare with pytest  
pytest --cov > pytest_cov.txt
veri --cov > veri_cov.txt
```

## Performance Expectations

### Typical Improvements

| Scenario | pytest | veri | Improvement |
|----------|---------|------|-------------|
| Full test run (cold) | 45s | 22s | 2x faster |
| Small change impact | 45s | 4s | 11x faster |
| Watch mode feedback | 8s | 0.3s | 27x faster |
| CI with 4 shards | 12m | 7m | 40% reduction |
| Coverage combine | 2m | 5s | 24x faster |

### When veri Might Be Slower

- **Very small test suites** (<10 tests): Overhead may not be worth it
- **First run**: Cache building adds initial overhead
- **100% test impact**: When all tests are affected by changes

## Configuration Examples

### Minimal Configuration
```toml
# pyproject.toml
[tool.veri]
workers = "auto"
coverage = true
```

### Development Team Configuration
```toml
# pyproject.toml
[tool.veri]
workers = 8
coverage = true
cov_report = ["html", "xml"]
watch_ignore = ["*.log", "tmp/", "build/"]

[tool.veri.ci]
shards = 4
junit_xml = "reports/junit-{shard}.xml"
```

### Enterprise Configuration
```toml
# pyproject.toml  
[tool.veri]
workers = 16
coverage = true
cov_diff_threshold = 85

[tool.veri.security]
enforce_allowlist = true
no_network = true
allowed_plugins = [
    "company-pytest-plugin==2.1.0",
    "internal-test-utils"
]

[tool.veri.telemetry]
enabled = false  # Explicitly disabled for compliance
```

## Getting Help

### Common Resources
- **Documentation**: [https://docs.veri.dev](https://docs.veri.dev)
- **GitHub Issues**: Report bugs and feature requests
- **Community Discord**: Real-time help and discussion

### Migration Support
- **Migration checklist**: Use this guide as a checklist
- **Performance benchmarking**: Use `scripts/bench.py` to measure improvements
- **Plugin compatibility**: Check allowlist and report issues

### Emergency Procedures

If you encounter critical issues during migration:

1. **Immediate rollback:**
   ```bash
   # Switch back to pytest temporarily
   pytest -n auto --cov
   ```

2. **Partial migration:**
   ```bash
   # Use veri for development, pytest for CI
   veri -w  # Local development
   pytest -n auto --cov  # CI (temporarily)
   ```

3. **Escape hatch:**
   ```bash
   # Use veri CLI but pytest engine
   veri --engine pytest --workers auto
   ```

Remember: veri is designed as a drop-in replacement, so reverting to pytest should always be straightforward if needed during the migration process.