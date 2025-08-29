"""pytest configuration for phase3 demo"""

import pytest

def pytest_configure(config):
    """Configure pytest with custom markers"""
    config.addinivalue_line("markers", "slow: marks tests as slow")
    config.addinivalue_line("markers", "edge_case: marks tests as edge cases")

@pytest.fixture
def sample_data():
    """Sample fixture for testing"""
    return {"x": 10, "y": 20}