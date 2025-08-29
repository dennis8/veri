"""Tests for pytest execution functionality."""



from veri_worker import VeriExecutor


class TestVeriExecutor:
    """Test cases for VeriExecutor."""

    def test_executor_initialization(self, temp_work_dir):
        """Test executor initialization."""
        executor = VeriExecutor(temp_work_dir)
        assert executor.work_dir == temp_work_dir

    def test_run_tests_empty_nodeids(self, temp_work_dir):
        """Test running with empty nodeids list."""
        executor = VeriExecutor(temp_work_dir)

        # Should return success for empty list
        exit_code = executor.run_tests([])
        assert exit_code == 0

    def test_run_passing_tests(self, temp_work_dir):
        """Test running passing tests."""
        # Create a simple passing test
        test_file = temp_work_dir / "test_passing.py"
        test_file.write_text('''
def test_pass():
    assert True

def test_also_pass():
    assert 1 == 1
''')

        executor = VeriExecutor(temp_work_dir)

        # Run specific nodeids
        nodeids = [
            "test_passing.py::test_pass",
            "test_passing.py::test_also_pass"
        ]

        exit_code = executor.run_tests(nodeids, quiet=True)
        assert exit_code == 0

    def test_run_failing_tests(self, temp_work_dir):
        """Test running failing tests."""
        # Create a failing test
        test_file = temp_work_dir / "test_failing.py"
        test_file.write_text('''
def test_fail():
    assert False, "This test should fail"

def test_pass():
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        # Run the failing test
        nodeids = ["test_failing.py::test_fail"]

        exit_code = executor.run_tests(nodeids, quiet=True)
        assert exit_code == 1  # pytest exit code for test failures

    def test_run_tests_with_verbose(self, temp_work_dir):
        """Test running tests with verbose output."""
        test_file = temp_work_dir / "test_verbose.py"
        test_file.write_text('''
def test_verbose():
    print("Verbose test output")
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        nodeids = ["test_verbose.py::test_verbose"]

        # Should not crash with verbose=True
        exit_code = executor.run_tests(nodeids, verbose=True, quiet=False)
        assert exit_code == 0

    def test_run_tests_with_exitfirst(self, temp_work_dir):
        """Test running tests with exitfirst option."""
        test_file = temp_work_dir / "test_exitfirst.py"
        test_file.write_text('''
def test_first_fail():
    assert False, "First failure"

def test_second():
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        nodeids = [
            "test_exitfirst.py::test_first_fail",
            "test_exitfirst.py::test_second"
        ]

        exit_code = executor.run_tests(nodeids, exitfirst=True, quiet=True)
        assert exit_code == 1  # Should fail and exit early

    def test_run_tests_with_maxfail(self, temp_work_dir):
        """Test running tests with maxfail option."""
        test_file = temp_work_dir / "test_maxfail.py"
        test_file.write_text('''
def test_fail_1():
    assert False, "Failure 1"

def test_fail_2():
    assert False, "Failure 2"

def test_pass():
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        nodeids = [
            "test_maxfail.py::test_fail_1",
            "test_maxfail.py::test_fail_2",
            "test_maxfail.py::test_pass"
        ]

        exit_code = executor.run_tests(nodeids, maxfail=1, quiet=True)
        assert exit_code == 1  # Should fail

    def test_run_pytest_engine(self, temp_work_dir):
        """Test pytest engine mode."""
        test_file = temp_work_dir / "test_engine.py"
        test_file.write_text('''
def test_engine():
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        # Test with basic args
        args = ["test_engine.py", "-v"]
        exit_code = executor.run_pytest_engine(args)
        assert exit_code == 0

    def test_run_pytest_engine_with_workers(self, temp_work_dir):
        """Test pytest engine with worker args."""
        test_file = temp_work_dir / "test_workers.py"
        test_file.write_text('''
def test_worker_1():
    assert True

def test_worker_2():
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        # Test with worker args (should be translated to -n)
        args = ["--workers", "2", "test_workers.py"]
        exit_code = executor.run_pytest_engine(args)
        assert exit_code == 0

    def test_run_pytest_engine_with_no_capture(self, temp_work_dir):
        """Test pytest engine with no-capture option."""
        test_file = temp_work_dir / "test_no_capture.py"
        test_file.write_text('''
def test_with_print():
    print("This should be captured")
    assert True
''')

        executor = VeriExecutor(temp_work_dir)

        # Test with no-capture
        args = ["--no-capture", "test_no_capture.py"]
        exit_code = executor.run_pytest_engine(args)
        assert exit_code == 0

    def test_nonexistent_test_file(self, temp_work_dir):
        """Test running tests with nonexistent file."""
        executor = VeriExecutor(temp_work_dir)

        # Try to run a nonexistent test
        nodeids = ["nonexistent.py::test_missing"]

        exit_code = executor.run_tests(nodeids, quiet=True)
        # Should return error code (not 0)
        assert exit_code != 0
