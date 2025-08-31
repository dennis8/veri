#!/usr/bin/env python3
# /// script
# dependencies = [
#   "pytest",
#   "coverage",
# ]
# ///
"""
veri Benchmark Harness

Comprehensive performance benchmarking for veri across multiple test suites,
comparing against pytest baselines and validating performance targets.

Usage:
    uv run scripts/bench.py --suite fastapi --scenarios cold,hot,watch
    uv run scripts/bench.py --all --output benchmarks/results.json
    uv run scripts/bench.py --regression-check baseline.json current.json
"""

import argparse
import json
import statistics
import subprocess
import tempfile
import time
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Dict, List, Optional, Any
import shutil
import sys
import platform
import os


@dataclass
class BenchmarkResult:
    """Single benchmark scenario result."""
    name: str
    mean: float
    median: float
    stdev: float
    min_time: float
    max_time: float
    runs: List[float]
    metadata: Dict[str, Any]


@dataclass
class BenchmarkSuite:
    """Complete benchmark suite results."""
    suite_name: str
    timestamp: str
    environment: Dict[str, str]
    scenarios: Dict[str, BenchmarkResult]
    targets_met: Dict[str, bool]
    summary: Dict[str, Any]


class BenchmarkRunner:
    """Main benchmark execution engine."""
    
    def __init__(self, repo_path: Path, runs: int = 5, verbose: bool = False):
        self.repo_path = repo_path
        self.runs = runs
        self.verbose = verbose
        self.results = {}
        
    def log(self, message: str):
        """Log message if verbose mode enabled."""
        if self.verbose:
            print(f"[BENCH] {message}")
            
    def ensure_clean_environment(self):
        """Clean all caches and temporary files."""
        self.log("Cleaning environment...")
        
        # Remove veri cache
        veri_cache = self.repo_path / ".veri"
        if veri_cache.exists():
            shutil.rmtree(veri_cache)
            
        # Remove pytest cache
        pytest_cache = self.repo_path / ".pytest_cache"
        if pytest_cache.exists():
            shutil.rmtree(pytest_cache)
            
        # Remove coverage files
        for cov_file in self.repo_path.glob(".coverage*"):
            cov_file.unlink()
            
        # Remove __pycache__ directories
        for pycache in self.repo_path.rglob("__pycache__"):
            if pycache.is_dir():
                shutil.rmtree(pycache)
                
    def run_command(self, command: List[str], timeout: int = 300) -> tuple[float, subprocess.CompletedProcess]:
        """Run a command and measure execution time."""
        self.log(f"Running: {' '.join(command)}")
        
        start_time = time.perf_counter()
        try:
            result = subprocess.run(
                command,
                cwd=self.repo_path,
                capture_output=True,
                text=True,
                timeout=timeout
            )
            end_time = time.perf_counter()
            execution_time = end_time - start_time
            
            if result.returncode != 0:
                self.log(f"Command failed with return code {result.returncode}")
                self.log(f"STDOUT: {result.stdout}")
                self.log(f"STDERR: {result.stderr}")
                
            return execution_time, result
            
        except subprocess.TimeoutExpired:
            self.log(f"Command timed out after {timeout} seconds")
            return float('inf'), None
            
    def benchmark_scenario(self, name: str, command: List[str], setup_fn=None, teardown_fn=None) -> BenchmarkResult:
        """Run a benchmark scenario multiple times and collect statistics."""
        self.log(f"Benchmarking scenario: {name}")
        
        times = []
        metadata = {'command': ' '.join(command), 'failures': 0}
        
        for run_idx in range(self.runs):
            self.log(f"Run {run_idx + 1}/{self.runs}")
            
            # Setup
            if setup_fn:
                setup_fn()
                
            # Execute and measure
            execution_time, result = self.run_command(command)
            
            if result and result.returncode == 0:
                times.append(execution_time)
            else:
                metadata['failures'] += 1
                self.log(f"Run {run_idx + 1} failed, skipping")
                
            # Teardown
            if teardown_fn:
                teardown_fn()
                
        if not times:
            raise RuntimeError(f"All runs failed for scenario: {name}")
            
        # Calculate statistics
        mean_time = statistics.mean(times)
        median_time = statistics.median(times)
        stdev_time = statistics.stdev(times) if len(times) > 1 else 0.0
        min_time = min(times)
        max_time = max(times)
        
        return BenchmarkResult(
            name=name,
            mean=mean_time,
            median=median_time,
            stdev=stdev_time,
            min_time=min_time,
            max_time=max_time,
            runs=times,
            metadata=metadata
        )
        
    def make_small_change(self) -> Path:
        """Make a small change to a leaf module."""
        # Find a suitable Python file to modify
        py_files = list(self.repo_path.rglob("*.py"))
        if not py_files:
            raise RuntimeError("No Python files found to modify")
            
        # Prefer files in src/ or similar directories
        target_file = None
        for py_file in py_files:
            if any(part in str(py_file) for part in ['src', 'lib', 'app']):
                target_file = py_file
                break
                
        if not target_file:
            target_file = py_files[0]
            
        # Add a small comment
        content = target_file.read_text()
        modified_content = content + f"\n# Benchmark modification {time.time()}\n"
        target_file.write_text(modified_content)
        
        self.log(f"Modified file: {target_file}")
        return target_file
        
    def make_large_change(self) -> Path:
        """Make a change that affects many tests."""
        # Look for core/common modules
        candidates = []
        for py_file in self.repo_path.rglob("*.py"):
            name = py_file.name.lower()
            if any(keyword in name for keyword in ['__init__', 'utils', 'common', 'base', 'core']):
                candidates.append(py_file)
                
        if not candidates:
            # Fall back to any Python file
            candidates = list(self.repo_path.rglob("*.py"))
            
        if not candidates:
            raise RuntimeError("No Python files found for large change")
            
        target_file = candidates[0]
        content = target_file.read_text()
        modified_content = f"# Large change {time.time()}\n" + content
        target_file.write_text(modified_content)
        
        self.log(f"Made large change to: {target_file}")
        return target_file


class TestSuiteManager:
    """Manages benchmark test suites (cloning, setup, etc.)."""
    
    SUITE_CONFIGS = {
        'fastapi': {
            'repo': 'https://github.com/tiangolo/fastapi.git',
            'branch': 'master',
            'test_dir': 'tests',
            'setup_commands': [
                ['uv', 'pip', 'install', '-e', '.'],
                ['uv', 'pip', 'install', '-r', 'requirements.txt']
            ]
        },
        'pydantic': {
            'repo': 'https://github.com/pydantic/pydantic.git', 
            'branch': 'main',
            'test_dir': 'tests',
            'setup_commands': [
                ['uv', 'pip', 'install', '-e', '.'],
                ['uv', 'pip', 'install', '-r', 'requirements/testing.txt']
            ]
        },
        'polars': {
            'repo': 'https://github.com/pola-rs/polars.git',
            'branch': 'main', 
            'test_dir': 'py-polars/tests',
            'setup_commands': [
                ['uv', 'pip', 'install', 'py-polars/']
            ]
        },
        'sqlalchemy': {
            'repo': 'https://github.com/sqlalchemy/sqlalchemy.git',
            'branch': 'main',
            'test_dir': 'test',
            'setup_commands': [
                ['uv', 'pip', 'install', '-e', '.']
            ]
        }
    }
    
    def __init__(self, workspace_dir: Path):
        self.workspace_dir = workspace_dir
        self.workspace_dir.mkdir(exist_ok=True)
        
    def setup_suite(self, suite_name: str) -> Path:
        """Clone and setup a test suite."""
        if suite_name not in self.SUITE_CONFIGS:
            raise ValueError(f"Unknown test suite: {suite_name}")
            
        config = self.SUITE_CONFIGS[suite_name]
        suite_dir = self.workspace_dir / suite_name
        
        # Clone if not exists
        if not suite_dir.exists():
            print(f"Cloning {suite_name}...")
            subprocess.run([
                'git', 'clone', '--depth', '1', 
                '--branch', config['branch'],
                config['repo'], str(suite_dir)
            ], check=True)
            
        # Setup dependencies
        print(f"Setting up {suite_name}...")
        for command in config['setup_commands']:
            subprocess.run(command, cwd=suite_dir, check=True)
            
        return suite_dir


class PerformanceValidator:
    """Validates benchmark results against performance targets."""
    
    # Performance targets from BENCHPLAN.md
    TARGETS = {
        'collection_speedup': 2.0,        # ≥2x faster collection
        'impact_analysis_ms': 100.0,      # ≤100ms impact analysis
        'watch_latency_ms': 300.0,        # ≤300ms watch latency
        'cpu_utilization': 0.9,           # ≥90% CPU utilization
        'ci_reduction': 0.2,               # ≥20% CI time reduction
        'coverage_speedup': 10.0,          # ≥10x coverage combine speedup
    }
    
    def validate_results(self, results: Dict[str, BenchmarkResult]) -> Dict[str, bool]:
        """Validate benchmark results against targets."""
        validation = {}
        
        # Collection speedup
        if 'cold_pytest' in results and 'cold_veri' in results:
            speedup = results['cold_pytest'].mean / results['cold_veri'].mean
            validation['collection_speedup'] = speedup >= self.TARGETS['collection_speedup']
            
        # Impact analysis speed
        if 'impact_analysis' in results:
            analysis_ms = results['impact_analysis'].mean * 1000
            validation['impact_analysis'] = analysis_ms <= self.TARGETS['impact_analysis_ms']
            
        # Watch mode latency
        if 'watch_latency' in results:
            latency_ms = results['watch_latency'].mean * 1000
            validation['watch_latency'] = latency_ms <= self.TARGETS['watch_latency_ms']
            
        return validation


class ReportGenerator:
    """Generates benchmark reports in various formats."""
    
    def __init__(self, output_dir: Path):
        self.output_dir = output_dir
        self.output_dir.mkdir(exist_ok=True)
        
    def generate_json_report(self, suite: BenchmarkSuite, filename: str):
        """Generate JSON report."""
        output_path = self.output_dir / filename
        
        # Convert dataclass to dict for JSON serialization
        report_data = {
            'suite_name': suite.suite_name,
            'timestamp': suite.timestamp,
            'environment': suite.environment,
            'scenarios': {name: asdict(result) for name, result in suite.scenarios.items()},
            'targets_met': suite.targets_met,
            'summary': suite.summary
        }
        
        with open(output_path, 'w') as f:
            json.dump(report_data, f, indent=2)
            
        print(f"JSON report written to: {output_path}")
        
    def generate_markdown_report(self, suite: BenchmarkSuite, filename: str):
        """Generate Markdown report."""
        output_path = self.output_dir / filename
        
        # Calculate improvements
        improvements = {}
        scenarios = suite.scenarios
        
        if 'cold_pytest' in scenarios and 'cold_veri' in scenarios:
            improvements['cold'] = scenarios['cold_pytest'].mean / scenarios['cold_veri'].mean
            
        if 'hot_pytest' in scenarios and 'hot_veri' in scenarios:
            improvements['hot'] = scenarios['hot_pytest'].mean / scenarios['hot_veri'].mean
            
        # Generate report
        report = f"""# veri Performance Report: {suite.suite_name}

**Generated**: {suite.timestamp}  
**Environment**: Python {suite.environment.get('python_version', 'unknown')}, {suite.environment.get('cpu_cores', 'unknown')} cores, {suite.environment.get('memory_gb', 'unknown')}GB RAM

## Summary

"""
        
        # Add target validation summary
        targets_met = sum(suite.targets_met.values())
        total_targets = len(suite.targets_met)
        
        if targets_met == total_targets:
            report += f"✅ **All {total_targets} targets met**\n"
        else:
            report += f"⚠️ **{targets_met}/{total_targets} targets met**\n"
            
        report += "\n## Performance Results\n\n"
        
        # Add detailed results
        for scenario_name, result in scenarios.items():
            if 'pytest' in scenario_name or 'veri' in scenario_name:
                continue  # Handle comparisons separately
                
            report += f"### {scenario_name.replace('_', ' ').title()}\n"
            report += f"- **Mean**: {result.mean:.3f}s ± {result.stdev:.3f}s\n"
            report += f"- **Median**: {result.median:.3f}s\n"
            report += f"- **Range**: {result.min_time:.3f}s - {result.max_time:.3f}s\n\n"
            
        # Add comparison table if we have pytest vs veri results
        if improvements:
            report += "## Performance Comparisons\n\n"
            report += "| Scenario | pytest | veri | Improvement |\n"
            report += "|----------|--------|------|-------------|\n"
            
            for scenario_type, improvement in improvements.items():
                pytest_key = f"{scenario_type}_pytest" 
                veri_key = f"{scenario_type}_veri"
                
                if pytest_key in scenarios and veri_key in scenarios:
                    pytest_time = scenarios[pytest_key].mean
                    veri_time = scenarios[veri_key].mean
                    
                    status = "✅" if improvement >= 2.0 else "⚠️" if improvement >= 1.5 else "❌"
                    
                    report += f"| {scenario_type.title()} | {pytest_time:.2f}s | {veri_time:.2f}s | {improvement:.1f}x {status} |\n"
                    
        with open(output_path, 'w') as f:
            f.write(report)
            
        print(f"Markdown report written to: {output_path}")


def get_environment_info() -> Dict[str, str]:
    """Collect environment information for benchmark context."""
    env_info = {
        'python_version': platform.python_version(),
        'platform': platform.platform(),
        'cpu_cores': str(os.cpu_count()),
        'memory_gb': 'unknown',  # Would need psutil for accurate memory info
    }
    
    # Try to get tool versions
    try:
        result = subprocess.run(['uv', 'run', 'pytest', '--version'], capture_output=True, text=True)
        if result.returncode == 0:
            env_info['pytest_version'] = result.stdout.split()[1]
    except:
        env_info['pytest_version'] = 'unknown'
        
    try:
        result = subprocess.run(['uv', 'tool', 'run', 'veri', '--version'], capture_output=True, text=True)
        if result.returncode == 0:
            env_info['veri_version'] = result.stdout.strip()
    except:
        env_info['veri_version'] = 'unknown'
        
    return env_info


def run_complete_benchmark(suite_name: str, scenarios: List[str], output_dir: Path, runs: int = 5):
    """Run complete benchmark suite."""
    print(f"Running benchmark for {suite_name} with scenarios: {scenarios}")
    
    # Setup test suite
    workspace = Path.cwd() / "benchmark_workspace"
    suite_manager = TestSuiteManager(workspace)
    repo_path = suite_manager.setup_suite(suite_name)
    
    # Initialize benchmark runner
    runner = BenchmarkRunner(repo_path, runs=runs, verbose=True)
    results = {}
    
    # Run scenarios
    if 'cold' in scenarios:
        print("\n=== Cold Run Benchmarks ===")
        
        # Cold pytest baseline
        runner.ensure_clean_environment()
        results['cold_pytest'] = runner.benchmark_scenario(
            'cold_pytest',
            ['uv', 'run', 'pytest', '-q', '--tb=no']
        )
        
        # Cold veri
        runner.ensure_clean_environment()
        results['cold_veri'] = runner.benchmark_scenario(
            'cold_veri', 
            ['uv', 'tool', 'run', 'veri', '-a', '-q']
        )
        
    if 'hot' in scenarios:
        print("\n=== Hot Run Benchmarks ===")
        
        # Setup: run once to warm caches
        runner.ensure_clean_environment()
        runner.run_command(['uv', 'tool', 'run', 'veri', '-a', '-q'])
        
        # Make small change
        changed_file = runner.make_small_change()
        
        # Hot pytest (full run)
        results['hot_pytest'] = runner.benchmark_scenario(
            'hot_pytest',
            ['uv', 'run', 'pytest', '-q', '--tb=no']
        )
        
        # Hot veri (impact-aware)
        results['hot_veri'] = runner.benchmark_scenario(
            'hot_veri',
            ['uv', 'tool', 'run', 'veri', '-q']
        )
        
    if 'watch' in scenarios:
        print("\n=== Watch Mode Benchmarks ===")
        
        # This would require more complex orchestration
        # For now, just measure impact analysis time
        runner.ensure_clean_environment()
        runner.run_command(['uv', 'tool', 'run', 'veri', '-a', '-q'])  # Warm cache
        
        changed_file = runner.make_small_change()
        
        results['impact_analysis'] = runner.benchmark_scenario(
            'impact_analysis',
            ['uv', 'tool', 'run', 'veri', '--explain', '--dry-run']  # Measure just the analysis
        )
        
    # Validate results
    validator = PerformanceValidator()
    targets_met = validator.validate_results(results)
    
    # Create benchmark suite
    suite = BenchmarkSuite(
        suite_name=suite_name,
        timestamp=time.strftime('%Y-%m-%d %H:%M:%S UTC', time.gmtime()),
        environment=get_environment_info(),
        scenarios=results,
        targets_met=targets_met,
        summary={
            'total_scenarios': len(results),
            'targets_met': sum(targets_met.values()),
            'total_targets': len(targets_met)
        }
    )
    
    # Generate reports
    reporter = ReportGenerator(output_dir)
    reporter.generate_json_report(suite, f"{suite_name}.json")
    reporter.generate_markdown_report(suite, f"{suite_name}.md")
    
    print(f"\nBenchmark complete. Results written to {output_dir}")
    return suite


def check_regression(baseline_file: Path, current_file: Path) -> List[Dict]:
    """Check for performance regressions."""
    with open(baseline_file) as f:
        baseline = json.load(f)
    with open(current_file) as f:
        current = json.load(f)
        
    regressions = []
    
    for scenario_name in baseline['scenarios']:
        if scenario_name not in current['scenarios']:
            continue
            
        baseline_time = baseline['scenarios'][scenario_name]['mean']
        current_time = current['scenarios'][scenario_name]['mean']
        
        regression_pct = (current_time - baseline_time) / baseline_time * 100
        
        if regression_pct > 10:  # 10% regression threshold
            regressions.append({
                'scenario': scenario_name,
                'baseline': baseline_time,
                'current': current_time,
                'regression_pct': regression_pct
            })
            
    return regressions


def main():
    parser = argparse.ArgumentParser(description='veri benchmark harness')
    parser.add_argument('--suite', choices=['fastapi', 'pydantic', 'polars', 'sqlalchemy'], 
                       help='Test suite to benchmark')
    parser.add_argument('--scenarios', default='cold,hot', 
                       help='Comma-separated list of scenarios (cold,hot,watch)')
    parser.add_argument('--runs', type=int, default=5, 
                       help='Number of runs per scenario')
    parser.add_argument('--output-dir', type=Path, default=Path('benchmarks'),
                       help='Output directory for results')
    parser.add_argument('--all', action='store_true',
                       help='Run all test suites')
    parser.add_argument('--regression-check', nargs=2, metavar=('BASELINE', 'CURRENT'),
                       help='Check for regressions between two result files')
    
    args = parser.parse_args()
    
    if args.regression_check:
        baseline_file, current_file = args.regression_check
        regressions = check_regression(Path(baseline_file), Path(current_file))
        
        if regressions:
            print("❌ Performance regressions detected:")
            for reg in regressions:
                print(f"  {reg['scenario']}: {reg['regression_pct']:.1f}% slower")
            sys.exit(1)
        else:
            print("✅ No significant performance regressions detected")
            sys.exit(0)
            
    scenarios = args.scenarios.split(',')
    
    if args.all:
        suites = ['fastapi', 'pydantic', 'polars', 'sqlalchemy']
    elif args.suite:
        suites = [args.suite]
    else:
        parser.error("Must specify --suite or --all")
        
    # Run benchmarks
    for suite in suites:
        try:
            run_complete_benchmark(suite, scenarios, args.output_dir, args.runs)
        except Exception as e:
            print(f"❌ Benchmark failed for {suite}: {e}")
            
    print("Benchmark harness complete!")


if __name__ == '__main__':
    main()
