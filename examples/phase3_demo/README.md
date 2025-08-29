# Phase 3 Demo Project

This is a simple Python project to demonstrate Phase 3 functionality:

- Test collection via pytest integration
- Test execution by nodeid
- pytest engine fallback mode

## Files

- `test_basic.py` - Basic test cases with different markers
- `test_calculator.py` - Simple calculator tests with parametrization
- `calculator.py` - Simple calculator module
- `conftest.py` - pytest configuration

## Usage

From the veri project root:

```bash
# Build veri
cargo build --workspace

# Collect tests (generates tests.index.json and markers.index.json)
target/debug/veri-cli -a --explain examples/phase3_demo/

# Run specific tests using veri engine
target/debug/veri-cli examples/phase3_demo/test_basic.py::test_addition

# Run tests with marker filter
target/debug/veri-cli -m slow examples/phase3_demo/

# Use pytest engine for full compatibility
target/debug/veri-cli --engine pytest examples/phase3_demo/
```