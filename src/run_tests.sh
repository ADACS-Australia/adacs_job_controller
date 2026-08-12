#!/bin/bash
#
# Test runner script for ADACS Job Controller (Rust)
#
# Usage:
#   ./run_tests.sh                              # Run all tests sequentially
#   ./run_tests.sh --verbose                    # Run with verbose output
#   ./run_tests.sh tests::job_tests             # Run specific test suite
#   ./run_tests.sh -- --nocapture               # Pass through to cargo test
#   ./run_tests.sh --coverage                   # Generate coverage report
#   ./run_tests.sh --coverage --open            # Generate and open coverage report
#   ./run_tests.sh -- --test-threads=4          # Override the thread count

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Check for --coverage flag
COVERAGE=false
OPEN_COVERAGE=false

FILTERED_ARGS=()

for arg in "$@"; do
    if [[ "$arg" == "--coverage" ]]; then
        COVERAGE=true
    elif [[ "$arg" == "--open" ]]; then
        OPEN_COVERAGE=true
    else
        FILTERED_ARGS+=("$arg")
    fi
done

# Tests share global state and must run sequentially, so enforce
# --test-threads=1 unless the caller explicitly passes their own.
TEST_THREADS_SET=false
for arg in "$@"; do
    if [[ "$arg" == *"--test-threads"* ]]; then
        TEST_THREADS_SET=true
        break
    fi
done

if [[ "$COVERAGE" == true ]]; then
    # Generate coverage report using cargo-llvm-cov
    echo "Running tests with coverage..."

    # Build base command with coverage options first, then test args
    COVERAGE_CMD="cargo llvm-cov --html"

    # Add any additional filtered args
    for arg in "${FILTERED_ARGS[@]}"; do
        COVERAGE_CMD="$COVERAGE_CMD $arg"
    done

    if [[ "$OPEN_COVERAGE" == true ]]; then
        COVERAGE_CMD="$COVERAGE_CMD --open"
    fi

    exec $COVERAGE_CMD
else
    # Run tests sequentially by default; respect an explicit --test-threads.
    if [[ "$TEST_THREADS_SET" == true ]]; then
        exec cargo test "$@"
    else
        ARGS=()
        DASH_DASH_SEEN=false
        INSERTED=false
        for arg in "$@"; do
            if [[ "$DASH_DASH_SEEN" == false && "$arg" == "--" ]]; then
                DASH_DASH_SEEN=true
                ARGS+=("$arg")
                ARGS+=("--test-threads=1")
                INSERTED=true
            else
                ARGS+=("$arg")
            fi
        done
        if [[ "$INSERTED" == false ]]; then
            ARGS+=("--")
            ARGS+=("--test-threads=1")
        fi
        exec cargo test "${ARGS[@]}"
    fi
fi
