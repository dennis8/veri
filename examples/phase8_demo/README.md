# Phase 8 Demo: CI Sharding & Artifacts

This directory demonstrates the Phase 8 CI sharding and artifacts functionality of veri.

## Features Demonstrated

### 1. Test Sharding Commands

- `veri split --ci N` - Split tests into N shards for CI
- `veri shard --ci I` - Get tests for shard I

### 2. Timing-Based Load Balancing

The sharding algorithm uses historical timing data to distribute tests evenly across shards based on execution time rather than just test count.

### 3. Event Streams

JSONL event streams for CI integration:
- `start` - Test run start event
- `plan` - Test plan with selected tests
- `case` - Individual test case results
- `summary` - Final run summary
- `log` - Log messages during execution

### 4. CI Templates

Ready-to-use CI pipeline templates are available in the `ci/` directory:
- GitHub Actions (`github-actions.yml`)
- GitLab CI (`gitlab-ci.yml`) 
- Azure Pipelines (`azure-pipelines.yml`)

## Usage Examples

### Basic Sharding

```bash
# Split tests into 4 shards
veri split --ci 4

# Get tests for shard 2 (0-indexed)
veri shard --ci 2

# Run shard 2 with event streaming
veri shard --ci 2 --stream-events events.jsonl
```

### With CI Integration

```bash
# In GitHub Actions
veri shard --ci ${{ strategy.job-index }} --stream-events shard-${{ strategy.job-index }}.jsonl

# In GitLab CI  
veri shard --ci $CI_NODE_INDEX --stream-events shard-$CI_NODE_INDEX.jsonl

# In Azure Pipelines
veri shard --ci $(System.JobPositionInPhase) --stream-events shard-$(System.JobPositionInPhase).jsonl
```

## Test Files

- `test_sharding.py` - Example tests with different execution times to demonstrate timing-based sharding
- Tests include slow, medium, and fast groups to show load balancing
- Parametrized tests demonstrate handling of test variants
- Marked tests show integration with pytest markers

## Expected Behavior

When running the sharding commands:

1. **Collection Phase**: Tests are discovered and collected with timing estimates
2. **Sharding Phase**: Tests are distributed across shards using bin-packing algorithm
3. **Execution Phase**: Each shard runs its assigned tests independently
4. **Reporting Phase**: Results are collected and can be merged for final reporting

The timing-based algorithm ensures that each shard has approximately equal execution time rather than just equal test counts.