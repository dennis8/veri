# veri Benchmarking Plan

This document outlines the comprehensive benchmarking strategy for veri, including methodologies, target metrics, test suites, and performance validation procedures.

## Overview

veri's performance claims are validated through rigorous benchmarking across diverse real-world codebases. Our benchmarks focus on the key areas where veri provides improvements: test collection, impact analysis, parallel execution, coverage processing, and CI workflows.

## Benchmark Categories

### 1. Collection Performance
**What we measure:** Time to discover and collect test cases
**Why it matters:** veri's single-collection model vs pytest's per-worker collection

### 2. Impact Analysis Speed  
**What we measure:** Time to compute affected tests after file changes
**Why it matters:** Enables sub-second feedback in watch mode

### 3. Execution Efficiency
**What we measure:** Wall-clock time and CPU utilization for test execution
**Why it matters:** Better scheduling and worker management

### 4. Coverage Processing
**What we measure:** Time to collect, combine, and report coverage data
**Why it matters:** veri's incremental coverage vs traditional combine

### 5. CI Workflows
**What we measure:** End-to-end CI execution time with sharding
**Why it matters:** Overall developer productivity and CI costs

## Test Suites

### Tier 1: Open Source Projects
Real-world codebases with diverse characteristics and public availability.

#### FastAPI (2,000+ tests)
- **Characteristics**: Web framework, async tests, fixtures
- **Test types**: Unit, integration, API tests
- **Complexity**: Moderate plugin usage, async patterns
- **Repository**: `https://github.com/tiangolo/fastapi`

#### Pydantic (3,000+ tests)  
- **Characteristics**: Data validation, heavy parametrization
- **Test types**: Property-based tests, performance tests
- **Complexity**: Complex type testing, JSON schema validation
- **Repository**: `https://github.com/pydantic/pydantic`

#### Polars (5,000+ tests)
- **Characteristics**: Data processing, performance-critical
- **Test types**: Dataframe operations, numerical tests
- **Complexity**: Large test matrix, performance benchmarks
- **Repository**: `https://github.com/pola-rs/polars`

#### SQLAlchemy (8,000+ tests)
- **Characteristics**: Database ORM, complex fixtures
- **Test types**: Database integration, dialect tests
- **Complexity**: Heavy fixture usage, database setup/teardown
- **Repository**: `https://github.com/sqlalchemy/sqlalchemy`

### Tier 2: Synthetic Benchmarks
Controlled test scenarios to isolate specific performance characteristics.

#### Large Monorepo Simulation
- **Scale**: 20,000+ tests across 500+ files
- **Structure**: Realistic import graph with multiple packages
- **Purpose**: Test scalability limits

#### Change Impact Scenarios
- **Leaf change**: Modify test file directly
- **Root change**: Modify core utility imported everywhere  
- **Conftest change**: Modify fixture definitions
- **Purpose**: Validate impact analysis accuracy and performance

#### Plugin Compatibility Matrix
- **Popular plugins**: pytest-django, pytest-asyncio, pytest-mock
- **Complex plugins**: pytest-benchmark, pytest-xdist compatibility
- **Purpose**: Ensure plugin ecosystem compatibility

## Benchmarking Methodology

### Environment Setup

#### Hardware Requirements
- **CPU**: 8+ cores (for parallel execution testing)
- **Memory**: 16GB+ (for large test suite processing)
- **Storage**: SSD (for fast cache I/O)
- **Network**: Stable connection (for CI simulation)

#### Software Environment
```bash
# Consistent Python environment
Python 3.11.x
uv 0.x.x
pytest 8.x.x
coverage 7.x.x

# Clean test environment
export PYTEST_DISABLE_PLUGIN_AUTOLOAD=1
export COVERAGE_PROCESS_START=
unset PYTHONPATH
```

#### Benchmark Harness

```python
# scripts/bench.py
import time
import subprocess
import statistics
from pathlib import Path

class BenchmarkRunner:
    def __init__(self, repo_path: Path, runs: int = 5):
        self.repo_path = repo_path
        self.runs = runs
        
    def benchmark_scenario(self, name: str, command: list[str]) -> dict:
        """Run a command multiple times and collect statistics."""
        times = []
        for i in range(self.runs):
            start = time.perf_counter()
            result = subprocess.run(command, cwd=self.repo_path, capture_output=True)
            end = time.perf_counter()
            
            if result.returncode != 0:
                raise RuntimeError(f"Command failed: {' '.join(command)}")
                
            times.append(end - start)
            
        return {
            'name': name,
            'mean': statistics.mean(times),
            'median': statistics.median(times),
            'stdev': statistics.stdev(times) if len(times) > 1 else 0,
            'min': min(times),
            'max': max(times),
            'runs': times
        }
```

### Test Scenarios

#### 1. Cold Full Run
**Purpose**: Measure initial collection and execution performance
```bash
# Clear all caches
rm -rf .veri .pytest_cache .coverage

# Pytest baseline
time pytest -q

# veri measurement  
time veri -a -q
```

#### 2. Hot Impact Run (Small Change)
**Purpose**: Measure impact analysis and selective execution
```bash
# Make small change to leaf module
echo "# comment" >> src/utils.py

# Pytest (full run)
time pytest -q

# veri (impact-aware)
time veri -q
```

#### 3. Hot Impact Run (Large Change)
**Purpose**: Measure behavior when many tests are affected
```bash
# Change core utility
echo "NEW_CONSTANT = 42" >> src/core.py

# Measure selection vs execution time
time veri --explain -q
```

#### 4. Watch Mode Performance
**Purpose**: Measure edit-to-feedback latency
```bash
# Start watch mode
veri -w &
PID=$!

# Simulate file edit
sleep 1
touch src/parser.py
# Measure time to first test result

kill $PID
```

#### 5. Parallel Execution Scaling
**Purpose**: Measure worker efficiency across different core counts
```bash
for workers in 1 2 4 8 16; do
    time veri --workers $workers -q
done
```

#### 6. Coverage Collection
**Purpose**: Measure incremental vs full coverage performance
```bash
# Full coverage collection
time veri -a --cov --cov-report xml -q

# Incremental coverage
echo "# comment" >> src/utils.py
time veri --cov --cov-report xml -q

# Coverage combine performance
time veri --cov-merge-full --cov-report xml -q
```

#### 7. CI Sharding Simulation
**Purpose**: Measure CI workflow performance
```bash
# Generate shards
time veri split --ci 4 > shards.json

# Run each shard (simulating parallel CI)
for shard in 0 1 2 3; do
    time veri shard --ci $shard --junit-xml junit-$shard.xml -q &
done
wait

# Combine results
time veri --cov-merge-full --cov-report xml
```

## Performance Targets

### Primary Metrics (P0 - Must Achieve)

#### Collection Performance
- **Target**: ≥2x faster than `pytest -n auto`
- **Measurement**: Time from start to first test execution
- **Rationale**: Single collection vs per-worker collection overhead

#### Impact Analysis Speed  
- **Target**: ≤100ms for impact computation on 10,000+ test suites
- **Measurement**: Time from file change detection to test selection
- **Rationale**: Enables sub-second watch mode feedback

#### Watch Mode Latency
- **Target**: ≤300ms from file save to first test failure
- **Measurement**: End-to-end latency in watch mode
- **Rationale**: Competitive with modern IDE feedback loops

### Secondary Metrics (P1 - Should Achieve)

#### Parallel Execution Efficiency
- **Target**: ≥90% CPU utilization with optimal worker count
- **Measurement**: CPU usage during test execution phase
- **Rationale**: Better than pytest-xdist's uneven load balancing

#### CI Wall-Clock Reduction
- **Target**: 20-40% faster CI execution with sharding
- **Measurement**: Total CI pipeline time
- **Rationale**: Significant impact on developer productivity

#### Coverage Processing Speed
- **Target**: ≥10x faster than `coverage combine`
- **Measurement**: Time to merge coverage data
- **Rationale**: Incremental processing vs full file parsing

### Aspirational Metrics (P2 - Nice to Have)

#### Memory Efficiency
- **Target**: ≤2x memory usage vs pytest
- **Measurement**: Peak RSS during execution
- **Rationale**: Caching overhead should be reasonable

#### Cache Effectiveness
- **Target**: ≥80% cache hit rate in typical development
- **Measurement**: Cache hits vs misses over development session
- **Rationale**: Cache should remain valid across most changes

## Measurement Framework

### Automated Benchmarking

```python
# scripts/bench.py implementation
def run_benchmark_suite(suite_name: str, repo_path: Path):
    """Run complete benchmark suite and generate report."""
    runner = BenchmarkRunner(repo_path)
    results = {}
    
    # Cold run benchmarks
    results['cold_pytest'] = runner.benchmark_scenario(
        'pytest_cold', ['pytest', '-q']
    )
    results['cold_veri'] = runner.benchmark_scenario(
        'veri_cold', ['veri', '-a', '-q']
    )
    
    # Hot impact benchmarks
    runner.make_small_change()
    results['hot_pytest'] = runner.benchmark_scenario(
        'pytest_hot', ['pytest', '-q']
    )
    results['hot_veri'] = runner.benchmark_scenario(
        'veri_hot', ['veri', '-q']
    )
    
    # Generate report
    generate_report(suite_name, results)

def generate_report(suite_name: str, results: dict):
    """Generate markdown report with performance comparison."""
    report = f"""# Benchmark Report: {suite_name}

## Cold Run Performance
| Tool | Mean (s) | Median (s) | Improvement |
|------|----------|------------|-------------|
| pytest | {results['cold_pytest']['mean']:.2f} | {results['cold_pytest']['median']:.2f} | baseline |
| veri | {results['cold_veri']['mean']:.2f} | {results['cold_veri']['median']:.2f} | {results['cold_pytest']['mean']/results['cold_veri']['mean']:.1f}x |

## Hot Run Performance (Small Change)
| Tool | Mean (s) | Median (s) | Improvement |
|------|----------|------------|-------------|
| pytest | {results['hot_pytest']['mean']:.2f} | {results['hot_pytest']['median']:.2f} | baseline |
| veri | {results['hot_veri']['mean']:.2f} | {results['hot_veri']['median']:.2f} | {results['hot_pytest']['mean']/results['hot_veri']['mean']:.1f}x |

"""
    Path(f"benchmarks/{suite_name}.md").write_text(report)
```

### CI Integration

```yaml
# .github/workflows/benchmarks.yml
name: Performance Benchmarks
on:
  schedule:
    - cron: '0 2 * * 0'  # Weekly Sunday 2 AM
  workflow_dispatch:

jobs:
  benchmark:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        suite: [fastapi, pydantic, polars, sqlalchemy]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v4
        with:
          python-version: '3.11'
      
      - name: Install tools
        run: |
          pip install uv
          uv tool install veri
          
      - name: Clone test suite
        run: scripts/clone_benchmark_suite.sh ${{ matrix.suite }}
        
      - name: Run benchmarks
        run: python scripts/bench.py ${{ matrix.suite }}
        
      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-${{ matrix.suite }}
          path: benchmarks/
```

## Benchmark Results Format

### JSON Output
```json
{
  "suite": "fastapi",
  "timestamp": "2025-08-30T10:41:03Z",
  "environment": {
    "python_version": "3.11.5",
    "pytest_version": "8.2.2", 
    "veri_version": "0.1.0",
    "platform": "linux-x86_64",
    "cpu_cores": 8,
    "memory_gb": 16
  },
  "scenarios": {
    "cold_full_run": {
      "pytest": {"mean": 45.2, "median": 44.8, "stdev": 1.3},
      "veri": {"mean": 22.1, "median": 21.9, "stdev": 0.8},
      "improvement": 2.04
    },
    "hot_impact_small": {
      "pytest": {"mean": 45.1, "median": 44.9, "stdev": 1.1}, 
      "veri": {"mean": 4.2, "median": 4.1, "stdev": 0.3},
      "improvement": 10.74
    },
    "watch_mode_latency": {
      "veri": {"mean": 0.28, "median": 0.27, "stdev": 0.02}
    }
  }
}
```

### Markdown Report
```markdown
# veri Performance Report: FastAPI

**Generated**: 2025-08-30 10:41:03 UTC  
**Environment**: Python 3.11.5, 8 cores, 16GB RAM  
**Test Suite**: FastAPI (2,347 tests)

## Summary

✅ **All targets met**  
- Collection: 2.0x faster (target: ≥2x)
- Impact analysis: 89ms (target: ≤100ms)  
- Watch latency: 280ms (target: ≤300ms)

## Detailed Results

### Cold Full Run
- **pytest**: 45.2s ± 1.3s
- **veri**: 22.1s ± 0.8s  
- **Improvement**: 2.0x faster ✅

### Hot Impact Run (Small Change)
- **pytest**: 45.1s ± 1.1s (full run)
- **veri**: 4.2s ± 0.3s (43 affected tests)
- **Improvement**: 10.7x faster ✅

### Watch Mode Latency
- **Edit to first failure**: 280ms ± 20ms ✅
```

## Regression Detection

### Performance Monitoring
```python
# scripts/check_regression.py
def check_performance_regression(baseline_path: Path, current_path: Path):
    """Compare current results against baseline and flag regressions."""
    baseline = json.loads(baseline_path.read_text())
    current = json.loads(current_path.read_text())
    
    regressions = []
    
    for scenario in baseline['scenarios']:
        if scenario not in current['scenarios']:
            continue
            
        baseline_time = baseline['scenarios'][scenario]['veri']['mean']
        current_time = current['scenarios'][scenario]['veri']['mean']
        
        regression_pct = (current_time - baseline_time) / baseline_time * 100
        
        if regression_pct > 10:  # 10% regression threshold
            regressions.append({
                'scenario': scenario,
                'baseline': baseline_time,
                'current': current_time,
                'regression_pct': regression_pct
            })
    
    return regressions
```

### CI Integration for Regression Detection
```yaml
- name: Check for performance regressions
  run: |
    python scripts/check_regression.py \
      benchmarks/baseline/fastapi.json \
      benchmarks/current/fastapi.json
  
- name: Comment on PR if regression detected
  if: failure()
  uses: actions/github-script@v6
  with:
    script: |
      github.rest.issues.createComment({
        issue_number: context.issue.number,
        owner: context.repo.owner,
        repo: context.repo.repo,
        body: '⚠️ Performance regression detected. See benchmark results.'
      })
```

## Continuous Benchmarking

### Weekly Performance Reports
- **Schedule**: Every Sunday at 2 AM UTC
- **Coverage**: All Tier 1 test suites
- **Output**: Performance dashboard and trend analysis
- **Alerts**: Automatic Slack/email on significant regressions

### Release Validation
- **Trigger**: Before each release
- **Scope**: Full benchmark suite including Tier 2 synthetic tests
- **Gate**: All P0 targets must be met for release approval
- **Documentation**: Performance characteristics documented in release notes

This benchmarking plan ensures veri's performance claims are rigorously validated and continuously monitored, providing confidence to users and maintainers about the tool's effectiveness.