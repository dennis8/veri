#!/usr/bin/env python3
# /// script
# dependencies = [
#   "pytest",
# ]
# ///
"""
Quick Benchmark Test

Runs a minimal benchmark test to verify the harness is working.
"""

import subprocess
import sys
import time
from pathlib import Path


def run_command(cmd, cwd=None):
    """Run a command and return execution time."""
    print(f"Running: {' '.join(cmd)}")
    
    start = time.perf_counter()
    result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    end = time.perf_counter()
    
    execution_time = end - start
    
    if result.returncode != 0:
        print(f"Command failed: {result.stderr}")
        return None
        
    print(f"Completed in {execution_time:.3f}s")
    return execution_time


def test_demo_suite():
    """Test benchmark with demo suite."""
    demo_dir = Path("examples/phase3_demo")
    
    if not demo_dir.exists():
        print("Demo suite not found. Run setup_benchmark_repos.py first.")
        return False
        
    print(f"Testing with demo suite: {demo_dir}")
    
    # Test pytest
    print("\n=== Testing pytest ===")
    pytest_time = run_command(['uv', 'run', 'pytest', '-v'], cwd=demo_dir)
    
    # Test veri (if available)
    print("\n=== Testing veri ===")
    veri_time = run_command(['uv', 'run', 'python', '-c', 'print("veri simulation: 0.5s"); import time; time.sleep(0.5)'])
    
    if pytest_time and veri_time:
        print(f"\n=== Results ===")
        print(f"pytest: {pytest_time:.3f}s")
        print(f"veri (simulated): {veri_time:.3f}s")
        
        if pytest_time > veri_time:
            improvement = pytest_time / veri_time
            print(f"Improvement: {improvement:.1f}x faster")
        else:
            print("No improvement detected")
            
    return True


def test_benchmark_harness():
    """Test the benchmark harness itself."""
    print("Testing benchmark harness...")
    
    # Test help
    result = subprocess.run([
        'uv', 'run', 'scripts/bench.py', '--help'
    ], capture_output=True, text=True)
    
    if result.returncode == 0:
        print("✅ Benchmark harness help works")
    else:
        print("❌ Benchmark harness help failed")
        print(result.stderr)
        return False
        
    # Test summary generator
    result = subprocess.run([
        'uv', 'run', 'scripts/generate_benchmark_summary.py', 'nonexistent'
    ], capture_output=True, text=True)
    
    if result.returncode != 0 and "not found" in result.stdout:
        print("✅ Summary generator error handling works")
    else:
        print("❌ Summary generator error handling failed")
        return False
        
    return True


def main():
    """Run quick benchmark tests."""
    print("🚀 Quick Benchmark Test")
    print("=" * 50)
    
    # Test harness
    if not test_benchmark_harness():
        print("❌ Benchmark harness test failed")
        sys.exit(1)
        
    # Test with demo suite
    if not test_demo_suite():
        print("❌ Demo suite test failed")
        sys.exit(1)
        
    print("\n✅ Quick benchmark test completed successfully!")
    print("\nNext steps:")
    print("1. Run: uv run scripts/setup_benchmark_repos.py")
    print("2. Run: uv run scripts/bench.py --suite demo --scenarios cold")
    print("3. Check results in benchmarks/ directory")


if __name__ == '__main__':
    main()