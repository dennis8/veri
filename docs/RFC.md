# RFC: veri - Ultra-fast, Impact-aware Test Runner

**Status**: Implemented  
**Author**: veri team  
**Created**: August 2025  
**Updated**: August 2025

## Summary

veri is a single-binary, pytest-compatible test runner that dramatically improves test execution speed through impact-aware test selection, efficient parallelization, and intelligent caching. It solves critical pain points in modern Python testing workflows while maintaining full compatibility with existing pytest-based test suites.

## Motivation

### Current Pain Points

Modern Python test suites face several critical performance bottlenecks:

1. **Per-worker collection overhead**: Tools like `pytest-xdist` re-collect tests in each worker process, creating O(n×workers) collection time
2. **Coarse change detection**: Developers run full test suites or rely on basic "last failed" filtering, missing optimal test selection
3. **Slow CI feedback**: Large test suites take 10-45 minutes in CI, slowing development velocity
4. **Coverage combining overhead**: `coverage combine` can take minutes on large codebases
5. **Watch mode inefficiency**: File watchers trigger broad test runs instead of minimal impacted sets

### Market Gap

Existing solutions are fragmented:
- **pytest-xdist**: Parallel execution but wasteful collection
- **pytest-testmon**: Runtime tracing with performance overhead  
- **pytest-split**: CI sharding but no local development benefits
- **pytest-watcher**: Watch mode but no impact analysis

No single tool provides fast impact analysis, efficient parallelization, and seamless CI integration.

## Design Principles

### 1. Performance First
- Sub-second feedback loops for small changes
- Minimal computational overhead  
- Efficient caching and incremental updates

### 2. Safety & Correctness
- Conservative static analysis (never miss tests)
- Deterministic test selection
- Graceful degradation under uncertainty

### 3. Drop-in Compatibility
- Works with existing pytest tests and configurations
- Familiar CLI interface and output formats
- Escape hatch to pure pytest when needed

### 4. Single Binary Philosophy
- No complex plugin dependencies
- Predictable behavior across environments
- Easy installation and deployment

## Architecture

### High-Level Design

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   CLI Frontend  │────│  Planner Engine  │────│ Worker Pool     │
│   (Rust)        │    │   (Rust)         │    │ (Python)        │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         │                       │                       │
         │              ┌──────────────────┐             │
         └──────────────│  Cache Layer     │─────────────┘
                        │  (JSON + SQLite) │
                        └──────────────────┘
```

### Core Components

#### 1. Static Analysis Engine
- **AST-based import parsing**: Fast, accurate dependency graphs
- **Module resolution**: PEP 420 namespace package support
- **Dynamic import detection**: Conservative broadening for safety

#### 2. Impact Analysis
- **Reverse dependency mapping**: Efficient transitive closure computation  
- **Change detection**: File system watching with Git awareness
- **Safety valves**: Automatic broadening when uncertainty detected

#### 3. Intelligent Scheduling
- **Historical timing data**: P95 duration-based prioritization
- **Bin-packing algorithm**: Optimal worker load balancing
- **Fail-first ordering**: Surface failures quickly

#### 4. Process Management
- **Sticky worker processes**: Amortized interpreter startup cost
- **Isolation boundaries**: Per-test process isolation option
- **Graceful cancellation**: Clean shutdown on watch mode edits

## Technical Specifications

### Cache Artifacts

veri maintains several JSON-schema validated caches:

```
.veri/cache/
├── tests.index.json       # file → [nodeids] mapping
├── module.map.json        # file → python.module.name  
├── imports.graph.json     # module → [imported_modules]
├── revdeps.graph.json     # module → [dependent_modules] 
├── fixtures.map.json      # conftest → scope mapping
├── markers.index.json     # nodeid → [markers]
├── timings.json          # nodeid → {mean_ms, p95_ms}
└── shards.manifest.json  # CI sharding configuration
```

### Cache Invalidation

Cache keys are computed from:
```python
cache_key = hash((
    python_version,
    platform, 
    veri_version,
    uv_lock_digest,
    site_packages_digest,
    pytest_version,
    plugins_versions,
    conftest_digests,
    veri_config_digest
))
```

### Impact Analysis Algorithm

```python
def compute_impacted_tests(changed_files: Set[Path]) -> Set[str]:
    """Conservative impact analysis with safety broadening."""
    
    # Direct test file changes
    test_files = {f for f in changed_files if is_test_file(f)}
    impacted = set().union(tests_index[f] for f in test_files)
    
    # Source file changes via reverse dependencies  
    source_files = {f for f in changed_files if f.suffix == '.py'} - test_files
    changed_modules = {module_map[f] for f in source_files}
    
    # Transitive closure of reverse dependencies
    dependent_modules = transitive_closure(changed_modules, revdeps_graph)
    dependent_files = {file_map[m] for m in dependent_modules if m in file_map}
    impacted.update(tests_index[f] for f in dependent_files if f in tests_index)
    
    # conftest.py changes affect directory subtrees
    conftest_changes = {f for f in changed_files if f.name == 'conftest.py'}
    for conftest in conftest_changes:
        impacted.update(tests_under_directory(conftest.parent))
    
    # Safety valve: broaden if uncertainty detected
    if has_dynamic_imports(changed_files) or len(impacted) > THRESHOLD * total_tests:
        return run_all_tests_safely()
        
    return impacted
```

## Implementation Strategy

### Phase 1: Core Engine (Weeks 1-4)
- Rust CLI with Python worker processes
- Basic AST parsing and dependency graph construction  
- Simple impact analysis and test execution
- File watching with debouncing

### Phase 2: Performance Optimization (Weeks 5-8)  
- Historical timing integration
- Intelligent scheduling algorithms
- Coverage integration with fast combining
- Windows compatibility and polish

### Phase 3: CI Integration (Weeks 9-12)
- Sharding manifest generation and consumption
- JUnit XML and JSONL output formats
- GitHub Actions, GitLab CI, Azure Pipelines templates  
- Plugin compatibility testing and auto-fallback

### Phase 4: Production Hardening (Weeks 13-16)
- Security model with plugin allowlisting
- Comprehensive error handling and diagnostics
- Performance benchmarking and optimization
- Documentation and migration guides

## Alternatives Considered

### 1. Pure pytest plugin approach
**Rejected**: Plugin ecosystem is fragmented, collection overhead remains unsolved

### 2. Runtime tracing (like testmon)
**Rejected**: Performance overhead, complex interaction with coverage tools

### 3. Git-based change detection  
**Rejected**: Insufficient for understanding Python import relationships

### 4. Full rewrite in Python
**Rejected**: Performance requirements favor compiled language for graph algorithms

## Security Considerations

### Plugin Security Model
- **Default allowlist**: Only vetted plugins run automatically
- **Manual override**: `--disable-allowlist` for controlled plugin testing  
- **Escape hatch**: `--engine pytest` for full compatibility mode

### Network Isolation
- **`--no-network` flag**: Block all network access during test execution
- **Sandboxing**: Optional container-based isolation for high-security environments

### Telemetry Privacy
- **Opt-in only**: No data collection without explicit user consent
- **Transparent**: Full disclosure of what data is collected
- **Multiple opt-out**: Supports DO_NOT_TRACK and other standards

## Performance Targets

### Benchmarks
Test against real-world codebases:
- **fastapi**: 2,000+ tests
- **pydantic**: 3,000+ tests  
- **polars**: 5,000+ tests
- **Large enterprise codebase**: 20,000+ tests

### Success Metrics
- **Collection**: ≥2x faster than `pytest -n auto`
- **Impact runs**: ≥10x faster than full runs on small changes
- **Watch feedback**: ≤300ms from file save to first failure
- **CI improvement**: 20-40% wall-clock time reduction

## Migration Strategy

### Compatibility Matrix
- ✅ **Core pytest**: fixtures, parametrize, markers, `-k` expressions
- ✅ **Framework integration**: Django, FastAPI, Flask, asyncio
- ✅ **Common plugins**: pytest-cov, pytest-mock, pytest-asyncio  
- ⚠️ **Complex plugins**: Auto-detect and graceful fallback

### Migration Path
1. **Drop-in replacement**: `pytest` → `veri` in most cases
2. **Gradual adoption**: Start with `veri -a`, add impact analysis
3. **CI integration**: Replace xdist + split plugins with native sharding
4. **Advanced features**: Add coverage gating, watch mode workflows

## Success Criteria

### Technical Goals
- **Correctness**: Never silently miss tests (verified via comprehensive test suite)
- **Performance**: Meet or exceed benchmark targets across test suites
- **Compatibility**: >95% of pytest test suites work without modification

### Adoption Goals  
- **Developer productivity**: Measurable improvement in feedback loops
- **CI efficiency**: Significant reduction in build times
- **Community adoption**: Positive reception from Python testing community

## Future Work

### v1.1+: Advanced Features
- **Remote caching**: Shared test results across developer machines
- **Differential coverage**: Focus on changed lines in PRs
- **Flaky test quarantine**: Automated flaky test management
- **Visual TUI**: Rich terminal interface for test execution

### v2.0+: Ecosystem Integration
- **IDE integration**: VS Code, PyCharm plugins for impact visualization
- **Cloud platforms**: Native support for GitHub Actions, etc.
- **ML-enhanced selection**: Learn from historical patterns to improve selection

## Conclusion

veri represents a significant advancement in Python testing tooling by solving fundamental performance and usability issues through careful engineering and principled design. The combination of static analysis, intelligent caching, and efficient execution provides substantial improvements while maintaining the familiar pytest experience developers expect.

The technical approach is proven by successful implementations in other ecosystems (e.g., Jest in JavaScript), and the conservative safety-first design ensures correctness even when facing edge cases like dynamic imports or complex plugin interactions.

By delivering veri as a single binary with minimal dependencies, we provide a tool that is both powerful and reliable, suitable for individual developers and large enterprise teams alike.