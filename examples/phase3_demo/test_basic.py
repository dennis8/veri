"""Basic test cases demonstrating different pytest features"""

import pytest
from calculator import add, subtract


def test_addition():
    """Basic addition test"""
    assert add(2, 3) == 5
    assert add(-1, 1) == 0
    assert add(0, 0) == 0


def test_subtraction():
    """Basic subtraction test"""
    assert subtract(5, 3) == 2
    assert subtract(1, 1) == 0
    assert subtract(0, 5) == -5


@pytest.mark.slow
def test_large_numbers():
    """Test with large numbers - marked as slow"""
    result = add(1000000, 2000000)
    assert result == 3000000


@pytest.mark.edge_case
def test_negative_numbers():
    """Test with negative numbers"""
    assert add(-5, -3) == -8
    assert subtract(-5, -3) == -2


class TestCalculatorClass:
    """Class-based tests"""

    def test_class_addition(self):
        """Addition test in class"""
        assert add(1, 2) == 3

    @pytest.mark.slow
    def test_class_complex(self):
        """Complex test in class"""
        result = add(subtract(10, 5), add(2, 3))
        assert result == 10
