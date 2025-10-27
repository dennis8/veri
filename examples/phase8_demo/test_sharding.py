"""
Example tests for demonstrating Phase 8 sharding functionality.
"""

import pytest
import time


class TestSlowGroup:
    """A group of slower tests for demonstrating timing-based sharding."""

    def test_slow_1(self):
        time.sleep(0.1)
        assert 2 + 2 == 4

    def test_slow_2(self):
        time.sleep(0.2)
        assert 3 * 3 == 9

    def test_slow_3(self):
        time.sleep(0.15)
        assert 5 + 5 == 10


class TestFastGroup:
    """A group of faster tests for demonstrating timing-based sharding."""

    def test_fast_1(self):
        assert 1 + 1 == 2

    def test_fast_2(self):
        assert 2 * 2 == 4

    def test_fast_3(self):
        assert 3 + 3 == 6

    def test_fast_4(self):
        assert 4 * 4 == 16


class TestMediumGroup:
    """A group of medium speed tests."""

    def test_medium_1(self):
        time.sleep(0.05)
        assert 10 // 2 == 5

    def test_medium_2(self):
        time.sleep(0.08)
        assert 12 % 5 == 2

    def test_medium_3(self):
        time.sleep(0.06)
        assert pow(2, 3) == 8


@pytest.mark.parametrize("value", [1, 2, 3, 4, 5])
def test_parametrized(value):
    """Parametrized test for demonstrating sharding of parametrized tests."""
    assert value > 0
    assert value <= 5


def test_standalone():
    """A standalone test outside of classes."""
    assert True


@pytest.mark.slow
def test_marked_slow():
    """Test with a slow marker."""
    time.sleep(0.3)
    assert "hello" + " " + "world" == "hello world"


@pytest.mark.integration
def test_integration():
    """Test with integration marker."""
    # Simulate integration test
    data = {"key": "value", "number": 42}
    assert data["key"] == "value"
    assert data["number"] == 42
