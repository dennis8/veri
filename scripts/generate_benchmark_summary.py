#!/usr/bin/env python3
# /// script
# dependencies = []
# ///
"""
Benchmark Summary Generator

Aggregates benchmark results from multiple test suites and generates
a comprehensive summary report.
"""

import json
import sys
from pathlib import Path
from typing import Dict, List


def load_benchmark_results(benchmark_dir: Path) -> Dict[str, dict]:
    """Load all benchmark JSON results from directory."""
    results = {}
    
    for json_file in benchmark_dir.glob("*.json"):
        suite_name = json_file.stem
        with open(json_file) as f:
            results[suite_name] = json.load(f)
            
    return results


def calculate_improvements(results: Dict[str, dict]) -> Dict[str, Dict[str, float]]:
    """Calculate performance improvements for each suite."""
    improvements = {}
    
    for suite_name, data in results.items():
        suite_improvements = {}
        scenarios = data.get('scenarios', {})
        
        # Cold run improvement
        if 'cold_pytest' in scenarios and 'cold_veri' in scenarios:
            pytest_time = scenarios['cold_pytest']['mean']
            veri_time = scenarios['cold_veri']['mean']
            suite_improvements['cold_improvement'] = pytest_time / veri_time
            
        # Hot run improvement
        if 'hot_pytest' in scenarios and 'hot_veri' in scenarios:
            pytest_time = scenarios['hot_pytest']['mean']
            veri_time = scenarios['hot_veri']['mean']
            suite_improvements['hot_improvement'] = pytest_time / veri_time
            
        # Impact analysis speed
        if 'impact_analysis' in scenarios:
            analysis_time_ms = scenarios['impact_analysis']['mean'] * 1000
            suite_improvements['impact_analysis_ms'] = analysis_time_ms
            
        improvements[suite_name] = suite_improvements
        
    return improvements


def generate_summary_table(results: Dict[str, dict], improvements: Dict[str, Dict[str, float]]) -> str:
    """Generate markdown summary table."""
    table = """| Suite | Tests | Cold Improvement | Hot Improvement | Impact Analysis | Targets Met |
|-------|-------|------------------|-----------------|-----------------|-------------|
"""
    
    for suite_name in sorted(results.keys()):
        data = results[suite_name]
        suite_improvements = improvements.get(suite_name, {})
        
        # Extract test count (would need to be added to benchmark results)
        test_count = "N/A"
        
        # Format improvements
        cold_imp = suite_improvements.get('cold_improvement', 0)
        cold_str = f"{cold_imp:.1f}x" if cold_imp > 0 else "N/A"
        
        hot_imp = suite_improvements.get('hot_improvement', 0)
        hot_str = f"{hot_imp:.1f}x" if hot_imp > 0 else "N/A"
        
        impact_ms = suite_improvements.get('impact_analysis_ms', 0)
        impact_str = f"{impact_ms:.0f}ms" if impact_ms > 0 else "N/A"
        
        # Targets met
        targets_met = data.get('targets_met', {})
        met_count = sum(targets_met.values()) if targets_met else 0
        total_count = len(targets_met) if targets_met else 0
        targets_str = f"{met_count}/{total_count}"
        
        # Add status indicators
        cold_status = "✅" if cold_imp >= 2.0 else "⚠️" if cold_imp >= 1.5 else "❌"
        hot_status = "✅" if hot_imp >= 5.0 else "⚠️" if hot_imp >= 2.0 else "❌"
        impact_status = "✅" if impact_ms <= 100 else "⚠️" if impact_ms <= 200 else "❌"
        
        table += f"| {suite_name} | {test_count} | {cold_str} {cold_status} | {hot_str} {hot_status} | {impact_str} {impact_status} | {targets_str} |\n"
        
    return table


def generate_detailed_results(results: Dict[str, dict]) -> str:
    """Generate detailed results section."""
    detailed = "\n## Detailed Results\n\n"
    
    for suite_name in sorted(results.keys()):
        data = results[suite_name]
        scenarios = data.get('scenarios', {})
        
        detailed += f"### {suite_name.title()}\n\n"
        
        # Environment info
        env = data.get('environment', {})
        detailed += f"**Environment**: Python {env.get('python_version', 'unknown')}, "
        detailed += f"{env.get('cpu_cores', 'unknown')} cores\n\n"
        
        # Scenario results
        for scenario_name, scenario_data in scenarios.items():
            if scenario_name.endswith('_pytest') or scenario_name.endswith('_veri'):
                continue  # Skip individual tool results, show comparisons instead
                
            detailed += f"**{scenario_name.replace('_', ' ').title()}**: "
            detailed += f"{scenario_data['mean']:.3f}s ± {scenario_data['stdev']:.3f}s\n\n"
            
        # Comparisons
        if 'cold_pytest' in scenarios and 'cold_veri' in scenarios:
            pytest_time = scenarios['cold_pytest']['mean']
            veri_time = scenarios['cold_veri']['mean']
            improvement = pytest_time / veri_time
            
            detailed += f"**Cold Run**: pytest {pytest_time:.2f}s → veri {veri_time:.2f}s "
            detailed += f"({improvement:.1f}x improvement)\n\n"
            
        if 'hot_pytest' in scenarios and 'hot_veri' in scenarios:
            pytest_time = scenarios['hot_pytest']['mean']
            veri_time = scenarios['hot_veri']['mean']
            improvement = pytest_time / veri_time
            
            detailed += f"**Hot Run**: pytest {pytest_time:.2f}s → veri {veri_time:.2f}s "
            detailed += f"({improvement:.1f}x improvement)\n\n"
            
    return detailed


def generate_performance_summary(results: Dict[str, dict]) -> str:
    """Generate overall performance summary."""
    if not results:
        return "No benchmark results found.\n"
        
    improvements = calculate_improvements(results)
    
    # Overall statistics
    cold_improvements = [imp.get('cold_improvement', 0) for imp in improvements.values() if imp.get('cold_improvement', 0) > 0]
    hot_improvements = [imp.get('hot_improvement', 0) for imp in improvements.values() if imp.get('hot_improvement', 0) > 0]
    
    summary = "# 📊 veri Performance Benchmark Summary\n\n"
    
    if cold_improvements:
        avg_cold = sum(cold_improvements) / len(cold_improvements)
        summary += f"**Average Collection Speedup**: {avg_cold:.1f}x (target: ≥2.0x)\n"
        
    if hot_improvements:
        avg_hot = sum(hot_improvements) / len(hot_improvements)
        summary += f"**Average Impact-Aware Speedup**: {avg_hot:.1f}x\n"
        
    # Targets summary
    total_targets = 0
    met_targets = 0
    
    for data in results.values():
        targets = data.get('targets_met', {})
        total_targets += len(targets)
        met_targets += sum(targets.values())
        
    if total_targets > 0:
        target_pct = (met_targets / total_targets) * 100
        summary += f"**Targets Met**: {met_targets}/{total_targets} ({target_pct:.0f}%)\n\n"
        
    # Summary table
    summary += generate_summary_table(results, improvements)
    
    # Detailed results
    summary += generate_detailed_results(results)
    
    # Performance notes
    summary += """
## Performance Notes

- **Cold runs** measure initial test collection and execution (cache cold)
- **Hot runs** measure impact-aware execution after small changes
- **Impact analysis** measures time to compute affected test set
- **Targets** are based on veri's performance goals (see BENCHPLAN.md)

## Interpretation

✅ **Target met** - Performance goal achieved  
⚠️ **Close to target** - Within reasonable range of goal  
❌ **Target missed** - Below performance expectations  

For detailed methodology and target definitions, see [BENCHPLAN.md](docs/BENCHPLAN.md).
"""
    
    return summary


def main():
    if len(sys.argv) != 2:
        print("Usage: uv run scripts/generate_benchmark_summary.py <benchmark_dir>")
        sys.exit(1)
        
    benchmark_dir = Path(sys.argv[1])
    
    if not benchmark_dir.exists():
        print(f"Benchmark directory not found: {benchmark_dir}")
        sys.exit(1)
        
    results = load_benchmark_results(benchmark_dir)
    summary = generate_performance_summary(results)
    
    print(summary)


if __name__ == '__main__':
    main()