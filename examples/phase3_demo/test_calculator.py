"""Calculator tests with parametrization"""

import pytest
from calculator import add, multiply, divide


@pytest.mark.parametrize(
    "a,b,expected",
    [
        (2, 3, 5),
        (0, 0, 0),
        (-1, 1, 0),
        (100, 200, 300),
        (1, 1, 2),  # Added new test case
    ],
)
def test_addition_parametrized(a, b, expected):
    """Parametrized addition tests"""
    assert add(a, b) == expected


@pytest.mark.parametrize(
    "a,b,expected",
    [
        (2, 3, 6),
        (5, 4, 20),
        (0, 100, 0),
        (-2, 3, -6),
    ],
)
def test_multiplication_parametrized(a, b, expected):
    """Parametrized multiplication tests"""
    assert multiply(a, b) == expected


@pytest.mark.parametrize(
    "a,b,expected",
    [
        (6, 2, 3.0),
        (10, 5, 2.0),
        (7, 2, 3.5),
    ],
)
def test_division_parametrized(a, b, expected):
    """Parametrized division tests"""
    assert divide(a, b) == expected


def test_division_by_zero():
    """Test division by zero raises exception"""
    with pytest.raises(ValueError, match="Cannot divide by zero"):
        divide(10, 0)
