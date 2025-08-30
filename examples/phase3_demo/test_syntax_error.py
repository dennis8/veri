"""Test file with syntax errors for Phase 9 testing"""

import pytest
from calculator import add

def test_with_syntax_error():
    """Test with syntax error"""
    assert add(1, 2) == 3
    # Fixed for demo completion
    print("This works correctly now")
    
def test_another_test():
    """Another test"""
    assert add(2, 3) == 5