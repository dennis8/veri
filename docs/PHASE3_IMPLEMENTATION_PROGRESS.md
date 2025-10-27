# Phase 3 Implementation Progress: Orchestration Decomposition

**Status**: 🔄 **IN PROGRESS** (Service Layer Complete, Refactoring in Progress)
**Date Started**: 2024
**Current Stage**: Service Trait & Implementation Creation (100% complete)
**Next Stage**: Integration into run_veri_engine()

---

## Completed: Service Layer Architecture ✅

All five orchestration services have been successfully created with trait definitions, default implementations, and mock implementations for testing:

### 1. ValidationOrchestrationService ✅
**File**: `crates/veri-cli/src/orchestrator/validation_service.rs`

**Responsibility**: Orchestrate environment validation before test execution

**Components**:
- `ValidationResult` struct with:
  - `valid: bool` - Whether environment is valid
  - `fallback_exit: Option<ExitCode>` - Early exit signal (e.g., fallback to pytest)
  - `diagnostics: Vec<String>` - Validation messages

- `ValidationOrchestrationService` trait defining `validate_environment()`

- `DefaultValidationService` implementation:
  - Checks compatibility and handles fallback to pytest
  - Validates Python environment
  - Scans pytest plugins and applies security allowlists
  - Prints compatibility reports when needed

- `MockValidationService` for testing

**Code Location**: Lines 1-158

---

### 2. CollectionOrchestrationService ✅
**File**: `crates/veri-cli/src/orchestrator/collection_service.rs`

**Responsibility**: Orchestrate test collection and caching strategy

**Components**:
- `CollectionOutcome` struct with:
  - `tests_index: TestsIndex` - Collected tests
  - `graphs: Option<(ImportsGraph, ReverseDepsGraph, ModuleMap)>` - Optional import graphs
  - `collection_time_ms: u64` - Timing information

- `CollectionOrchestrationService` trait defining `collect_or_load()`

- `DefaultCollectionService` implementation:
  - Determines if collection is needed (--all or cache invalid)
  - Executes test collection via PythonWorker
  - Builds import graphs conditionally (only when not --all)
  - Tracks timing metrics
  - Handles collection errors

- `MockCollectionService` for testing

**Design Note**: Collection happens on-demand with configurable graph building based on impact analysis needs.

**Code Location**: Lines 1-184

---

### 3. SelectionOrchestrationService ✅
**File**: `crates/veri-cli/src/orchestrator/selection_orchestrator.rs`

**Responsibility**: Orchestrate test selection with impact analysis

**Components**:
- `SelectionOutcome` struct with:
  - `nodeids: Vec<String>` - Tests to run
  - `stats: SelectionStats` - Selection metrics (total, selected, fallback status)
  - `early_exit: Option<ExitCode>` - Early exit signal (e.g., no tests found)

- `SelectionOrchestrationService` trait defining `determine_tests_to_run()`

- `DefaultSelectionOrchestrator` implementation:
  - Handles --all flag (run all tests)
  - Loads or builds import graphs for impact analysis
  - Gets changed files from VCS
  - Applies selection criteria (keywords, markers, paths)
  - Implements fallback logic when no tests selected
  - Detects no-tests-found condition

- `MockSelectionOrchestrator` for testing

**Design Note**: Clear separation between --all path (simple) and impact analysis path (complex).

**Code Location**: Lines 1-201

---

### 4. ExecutionOrchestrationService ✅
**File**: `crates/veri-cli/src/orchestrator/execution_orchestrator.rs`

**Responsibility**: Coordinate test execution with coverage

**Components**:
- `ExecutionOutcome` struct with:
  - `result: ExecutionResult` - Test execution results
  - `coverage_collected: bool` - Whether coverage was collected

- `ExecutionOrchestrationService` trait defining `execute_with_coverage()`

- `DefaultExecutionOrchestrator` implementation:
  - Parses worker count (handles "auto", numeric, defaults)
  - Builds test run options with coverage configuration
  - Initializes coverage service
  - Executes tests via ExecutionService
  - Finalizes coverage collection

- `MockExecutionOrchestrator` for testing with:
  - `with_success()` - Pre-configured success result
  - `with_failure()` - Pre-configured failure result

**Code Location**: Lines 1-189

---

### 5. TelemetryOrchestrationService ✅
**File**: `crates/veri-cli/src/orchestrator/telemetry_orchestrator.rs`

**Responsibility**: Aggregate and record execution metrics

**Components**:
- `TelemetryOrchestrationService` trait defining `record_execution_metrics()`

- `DefaultTelemetryOrchestrator` implementation:
  - Computes config digest and cache key
  - Builds RunEvent from execution results
  - Records features used (coverage, watch, parallel, impact_analysis)
  - Records errors for failed runs
  - Includes Python version in metrics

- `MockTelemetryOrchestrator` for testing (no-op implementation)

**Code Location**: Lines 1-115

---

## Service Architecture Validation ✅

All services:
- ✅ Implement `Send + Sync` for thread-safety
- ✅ Have trait-based abstraction for testability
- ✅ Include mock implementations for unit testing
- ✅ Use proper error handling with `Result<T>`
- ✅ Compile without errors (22 warnings about unused code, expected)

---

## Next Steps: Integration into run_veri_engine()

The refactoring of `run_veri_engine()` will proceed in the following stages:

### Stage 1: Import New Services
```rust
use super::validation_service::{ValidationOrchestrationService, DefaultValidationService};
use super::collection_service::{CollectionOrchestrationService, DefaultCollectionService};
use super::selection_orchestrator::{SelectionOrchestrationService, DefaultSelectionOrchestrator};
use super::execution_orchestrator::{ExecutionOrchestrationService, DefaultExecutionOrchestrator};
use super::telemetry_orchestrator::{TelemetryOrchestrationService, DefaultTelemetryOrchestrator};
```

### Stage 2: Refactor run_veri_engine() (Target: ~80-100 lines)

Replace the current 286-line monolithic function with:

```rust
pub(super) fn run_veri_engine(
    cli: &Cli,
    config: &Config,
    security_config: &SecurityConfig,
    compatibility_matrix: &CompatibilityMatrix,
    telemetry: &mut TelemetryService,
    watch_adapter: &dyn WatchAdapter,
) -> Result<ExitCode> {
    println!("🚀 Using veri engine for maximum speed");

    // Setup
    let (work_dir, cache_dir, services, python_runtime) = setup_orchestration(
        cli, config, telemetry, watch_adapter,
    )?;

    // 1. Validate environment
    let validation_svc = DefaultValidationService::new();
    let validation = validation_svc.validate_environment(
        cli, config, &worker, compatibility_matrix, security_config, telemetry, &mut diagnostics,
    )?;
    if let Some(exit) = validation.fallback_exit {
        return Ok(exit);
    }

    // 2. Collect or load tests
    let collection_svc = DefaultCollectionService::new(&work_dir, &cache_dir, python_runtime);
    let collection = collection_svc.collect_or_load(
        cli.all, &collection_paths, &cli.ignore, &mut diagnostics,
    )?;
    let (tests_index, graphs, collection_time_ms) = (
        collection.tests_index,
        collection.graphs,
        collection.collection_time_ms,
    );

    // 3. Select tests to run
    let selection_svc = DefaultSelectionOrchestrator::new(&work_dir, &cache_dir, python_runtime);
    let selection = selection_svc.determine_tests_to_run(
        &tests_index, cli, &services, graphs.as_ref().map(|(ig, rg, mm)| (ig, rg, mm)),
    )?;
    if let Some(exit) = selection.early_exit {
        return Ok(exit);
    }

    // 4. Execute tests with coverage
    let execution_svc = DefaultExecutionOrchestrator::new(&work_dir, &cache_dir);
    let execution = execution_svc.execute_with_coverage(
        &selection.nodeids, &tests_index, cli, &services,
    )?;

    // 5. Record telemetry
    let telemetry_svc = DefaultTelemetryOrchestrator::new(python_runtime);
    telemetry_svc.record_execution_metrics(
        &execution.result, collection_time_ms, cli, config, telemetry, needs_collection,
    )?;

    Ok(execution.result.exit_code)
}
```

### Stage 3: Update Module Exports

Add public exports to make services accessible:
```rust
// In mod.rs or services.rs
pub use validation_service::{ValidationOrchestrationService, DefaultValidationService};
pub use collection_service::{CollectionOrchestrationService, DefaultCollectionService};
pub use selection_orchestrator::{SelectionOrchestrationService, DefaultSelectionOrchestrator};
pub use execution_orchestrator::{ExecutionOrchestrationService, DefaultExecutionOrchestrator};
pub use telemetry_orchestrator::{TelemetryOrchestrationService, DefaultTelemetryOrchestrator};
```

### Stage 4: Add Comprehensive Tests

Create `crates/veri-cli/src/orchestrator/orchestration_tests.rs` with tests for:

**Service Integration Tests**:
- Test full pipeline with mocked services
- Verify data flows correctly between stages
- Test error handling at each stage
- Verify early exits work correctly

**Edge Case Tests**:
- No tests found scenario
- Compatibility fallback triggered
- Plugin validation failures
- Coverage collection enabled/disabled
- Parallel worker count variations

**Before/After Comparison Tests**:
- Run same test scenarios with old and new orchestration
- Verify identical output and behavior
- Performance regression detection

---

## Expected Outcomes After Phase 3

### Code Reduction

**Before Phase 3**:
- `run_veri_engine()`: ~286 lines
- Orchestration logic: Mixed and interleaved
- Service layer: Thin (execution, selection only)

**After Phase 3**:
- `run_veri_engine()`: ~80-100 lines (70% reduction)
- Orchestration logic: Clear, linear flow
- Service layer: Rich with 5-6 focused services
- Each service: 50-150 lines with clear responsibility

### Quality Improvements

| Metric | Improvement |
|--------|-------------|
| **Readability** | Clear pipeline stages visible at top level |
| **Maintainability** | Each service independently understandable |
| **Testability** | Each service tested in isolation + integration |
| **Extensibility** | Easy to add new pipeline stages |
| **Error Handling** | Clear per-stage error propagation |
| **Documentation** | Self-documenting service names |

### Test Coverage Impact

Current: 62/62 tests passing (100%)
Target: Maintain 100%, add 15-20 new integration tests

### Compilation

Current: Compiles with 22 warnings (unused code, expected)
Target: Zero warnings when services are integrated

---

## Implementation Checklist

### Phase 3.1: Service Creation ✅ COMPLETE
- [x] Create `validation_service.rs` with trait and implementations
- [x] Create `collection_service.rs` with trait and implementations
- [x] Create `selection_orchestrator.rs` with trait and implementations
- [x] Create `execution_orchestrator.rs` with trait and implementations
- [x] Create `telemetry_orchestrator.rs` with trait and implementations
- [x] Add modules to `mod.rs`
- [x] Verify compilation (cargo check)

### Phase 3.2: Integration (IN PROGRESS)
- [ ] Import services in `internals.rs`
- [ ] Refactor `run_veri_engine()` to use services
- [ ] Extract helper functions (setup_orchestration, etc.)
- [ ] Remove old inline code
- [ ] Verify no behavior changes
- [ ] Fix remaining compilation warnings

### Phase 3.3: Testing (PENDING)
- [ ] Add integration tests
- [ ] Test error scenarios
- [ ] Test edge cases
- [ ] Verify performance (no regression)
- [ ] Run full test suite

### Phase 3.4: Documentation (PENDING)
- [ ] Update PHASE3_ORCHESTRATION_DECOMPOSITION_PLAN.md with final results
- [ ] Document each service's contract
- [ ] Add architecture diagrams
- [ ] Create PHASE3_COMPLETION_ANALYSIS.md

---

## Files Created This Session

1. `crates/veri-cli/src/orchestrator/validation_service.rs` - 158 lines
2. `crates/veri-cli/src/orchestrator/collection_service.rs` - 184 lines
3. `crates/veri-cli/src/orchestrator/selection_orchestrator.rs` - 201 lines
4. `crates/veri-cli/src/orchestrator/execution_orchestrator.rs` - 189 lines
5. `crates/veri-cli/src/orchestrator/telemetry_orchestrator.rs` - 115 lines

**Total**: 847 lines of service layer code with proper testing infrastructure

## Files Modified This Session

1. `crates/veri-cli/src/orchestrator/mod.rs` - Added module declarations
2. `crates/veri-cli/src/orchestrator/execution.rs` - Added Clone + Debug to ExecutionResult

---

## Recommendations for Next Session

1. **High Priority**: Complete integration of services into `run_veri_engine()`
2. **High Priority**: Add integration tests to verify pipeline works correctly
3. **Medium Priority**: Remove old inline functions from internals.rs
4. **Medium Priority**: Update documentation with Phase 3 completion analysis
5. **Low Priority**: Performance profiling to ensure no regression

---

## Notes

- All services compile without errors
- Service mocks are functional for unit testing
- Traits are properly designed for testability
- No breaking changes to existing interfaces yet
- Ready for integration phase in next session

---

**Session Status**: Service layer architecture complete and validated
**Next Session**: Integration + Testing + Documentation
**Estimated Time for Phase 3 Completion**: 2-3 more hours

---

Generated during Phase 3 Orchestration Decomposition implementation.
For detailed plan, see: docs/PHASE3_ORCHESTRATION_DECOMPOSITION_PLAN.md
