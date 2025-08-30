# Veri CI Templates

This directory contains ready-to-use CI templates for integrating Veri's sharding capabilities into your continuous integration pipeline.

## Overview

Veri provides built-in CI sharding that distributes tests across multiple workers using intelligent timing-based bin-packing. This reduces CI wall-clock time by running tests in parallel while maintaining load balance.

### Key Benefits

- **Balanced Shards**: Uses historical timing data for optimal distribution
- **Stable Assignment**: Same tests always go to same shard for consistent caching
- **Fast Feedback**: Failed tests prioritized to surface issues quickly
- **Standard Output**: JUnit XML and JSONL event streams for CI integration

## Quick Start

### 1. Basic Usage

```bash
# Generate manifest for 4 shards
veri split --ci 4 > shards.json

# Run a specific shard  
veri shard --ci 0 --manifest shards.json --junit-xml reports/junit.xml
```

### 2. CI Integration

Choose your CI platform and copy the appropriate template:

- **GitHub Actions**: `github-actions.yml` → `.github/workflows/test.yml`
- **GitLab CI**: `gitlab-ci.yml` → `.gitlab-ci.yml`  
- **Azure Pipelines**: `azure-pipelines.yml` → `azure-pipelines.yml`

### 3. Configuration

All templates follow the same pattern:

1. **Prepare Stage**: Generate sharding manifest
2. **Test Stage**: Run tests in parallel shards
3. **Coverage Stage**: Merge coverage reports

## Template Details

### GitHub Actions (`github-actions.yml`)

Features:
- Dynamic matrix generation from shards manifest
- Artifact sharing between jobs
- Test result reporting via `dorny/test-reporter`
- Coverage upload to Codecov
- Proper caching of veri data

Usage:
```yaml
# Copy to .github/workflows/test.yml
# Customize Python version, shard count, and test commands as needed
```

### GitLab CI (`gitlab-ci.yml`)

Features:
- Template-based job definition for easy customization
- Built-in JUnit test reporting
- Coverage parsing and visualization
- Manual codecov upload job

Usage:
```yaml
# Copy to .gitlab-ci.yml
# Adjust shard count by modifying the shard job definitions
```

### Azure Pipelines (`azure-pipelines.yml`)

Features:
- Matrix strategy for parallel shard execution
- Native test result publishing
- Coverage result publishing
- Artifact management

Usage:
```yaml
# Copy to azure-pipelines.yml
# Modify matrix strategy to change shard count
```

## Customization

### Changing Shard Count

Modify the number in the split command:

```bash
# For 8 shards instead of 4
veri split --ci 8 > shards.json
```

Then update your CI template to include all shard IDs (0-7).

### Test Selection

Add filters to shard execution:

```bash
# Run only specific markers in shard
veri shard --ci 0 --manifest shards.json -m "integration"

# Run specific test paths in shard  
veri shard --ci 0 --manifest shards.json tests/integration/
```

### Output Configuration

Customize reporting formats:

```bash
veri shard --ci 0 --manifest shards.json \
  --junit-xml reports/junit.xml \
  --jsonl reports/events.jsonl \
  --cov \
  --cov-merge-full
```

### Performance Tuning

#### Cache Optimization

Ensure veri cache is preserved between runs:

```yaml
- name: Cache veri data
  uses: actions/cache@v4
  with:
    path: .veri/cache
    key: veri-cache-${{ runner.os }}-${{ hashFiles('**/pyproject.toml') }}
```

#### Historical Timing Data

For best shard balance, ensure timing data is available:

1. Run full test suite at least once to collect timing data
2. Include `.veri/cache/timings.json` in your cache
3. Timing data improves with each CI run

#### Load Balance Monitoring

Check shard balance with `--explain`:

```bash
veri split --ci 4 --explain > shards.json
```

Target: >90% balance ratio for optimal performance.

## Event Stream Format

The JSONL event stream provides detailed CI analytics:

```json
{"type":"start","timestamp":"2024-01-01T00:00:00Z","run_id":"veri-123","total_tests":100,"selected_tests":25,"workers":1,"strategy":"shard"}
{"type":"plan","timestamp":"2024-01-01T00:00:00Z","run_id":"veri-123","shard_id":0,"selected_nodeids":["test1","test2"],"estimated_duration":30.5}
{"type":"case","timestamp":"2024-01-01T00:00:00Z","run_id":"veri-123","shard_id":0,"nodeid":"test1","outcome":"passed","duration":1.2}
{"type":"summary","timestamp":"2024-01-01T00:00:00Z","run_id":"veri-123","shard_id":0,"total_duration":35.0,"tests_run":25,"tests_passed":24,"tests_failed":1,"exit_code":1}
```

Use this data for:
- CI dashboard visualization
- Performance monitoring
- Flaky test detection
- Historical trend analysis

## Troubleshooting

### Manifest Validation Errors

```bash
# Check manifest format
veri shard --ci 0 --manifest shards.json --explain
```

Common issues:
- Wrong format version (must be `veri-shards@1`)
- Missing shard IDs (must be sequential 0, 1, 2, ...)
- Tests in manifest not found in current suite

### Load Imbalance

If shards have significantly different durations:

1. Check if timing data exists: `ls .veri/cache/timings.json`
2. Run a full collection first: `veri -a`
3. Consider increasing shard count for better granularity
4. Check for extremely long-running tests that dominate shards

### Coverage Issues

For accurate coverage merging:

1. Ensure all shards use `--cov`
2. Use `--cov-merge-full` only in final merge step
3. Verify all coverage artifacts are collected
4. Check that source paths are consistent across shards

### Performance Not Meeting Expectations

Expected improvements:
- **20-30% CI wall-clock reduction** for balanced test suites
- **Near-linear scaling** up to CPU core count
- **Sub-second overhead** for shard startup

If not seeing improvements:
1. Verify tests are CPU-bound, not I/O-bound
2. Check for unbalanced test timing distribution
3. Ensure sufficient test volume (>100 tests minimum)
4. Consider test containerization for I/O isolation

## Migration from pytest-xdist/pytest-split

### From pytest-xdist

Replace:
```bash
pytest -n auto
```

With:
```bash
veri split --ci $(nproc) > shards.json
veri shard --ci $SHARD_ID --manifest shards.json
```

### From pytest-split

Replace:
```bash
pytest --splits=4 --group=$GROUP
```

With:
```bash
veri split --ci 4 > shards.json  
veri shard --ci $(($GROUP-1)) --manifest shards.json
```

Key advantages of veri:
- Single binary (no plugin dependencies)
- Stable shard assignment (better caching)
- Timing-aware load balancing
- Built-in coverage optimization

## Support

For questions or issues:

1. Check the [main documentation](../README.md)
2. Review [troubleshooting guide](../docs/TROUBLESHOOTING.md)
3. File issues on the [GitHub repository](https://github.com/veri-dev/veri)

Example working configurations are available in the [examples directory](../examples/).