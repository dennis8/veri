# veri Schemas

This directory contains JSON schemas for all veri artifacts. These schemas define stable contracts for cache files and event streams, enabling integration with external tools and validation of generated data.

## Schema Files

### Core Artifacts
- **`tests.index.json`** - Index of all collected test nodeids with metadata (markers, fixtures, parametrization)
- **`module.map.json`** - Mapping between file paths and Python module names (supports PEP 420 namespace packages)
- **`imports.graph.json`** - Directed graph of import dependencies between modules
- **`revdeps.graph.json`** - Reverse dependency mapping for efficient impact analysis
- **`fixtures.map.json`** - Mapping of pytest fixtures and their dependencies
- **`markers.index.json`** - Index of pytest markers and their usage patterns

### Performance & Timing
- **`timings.json`** - Historical test execution timing data with aggregated statistics

### Event Streaming
- **`event.jsonl.json`** - Schema for individual event lines in JSONL stream (start, plan, case, summary, log events)

### CI Integration
- **`shards.manifest.json`** - CI sharding manifest for distributing tests across workers (`veri-shards@1` format)

## Schema Versioning

All schemas follow semantic versioning and include a `version` field. The current schema version is `0.1.0`.

## Validation

Schema validation can be performed using any JSON Schema validator. For CI validation:

```bash
# Install a JSON Schema validator
npm install -g ajv-cli

# Validate generated artifacts against schemas
ajv validate -s schemas/tests.index.json -d .veri/cache/tests.index
ajv validate -s schemas/event.jsonl.json -d .veri/events.jsonl --verbose
```

## Usage in External Tools

These schemas enable integration with various tools:

- **CI dashboards** - Parse JSONL events for real-time test reporting
- **Coverage tools** - Use module maps and dependency graphs
- **Test analytics** - Consume timing data and test outcomes
- **Shard balancing** - Use shard manifests for optimal CI distribution
