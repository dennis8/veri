"""Simple calculator for testing purposes"""


def add(a, b):
    """Add two numbers (touched 2025-08-31 for impact test)"""
    # Enhanced for Phase 9 testing; minor edit to trigger impact analysis
    return a + b


def subtract(a, b):
    """Subtract two numbers"""
    return a - b


def multiply(a, b):
    """Multiply two numbers"""
    return a * b


def divide(a, b):
    """Divide two numbers"""
    if b == 0:
        raise ValueError("Cannot divide by zero")
    return a / b  # Test change
