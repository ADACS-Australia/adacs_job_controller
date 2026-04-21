#!/bin/bash
set -e

cd /app

# Wait for MySQL to be available
echo "Waiting for MySQL..."
until nc -z "${DATABASE_HOST:-db}" "${DATABASE_PORT:-3306}"; do
    echo "  MySQL not ready, retrying in 2s..."
    sleep 2
done
echo "MySQL is ready."

# Create test report directory
mkdir -p /app/test_report

echo "=== Running unit tests ==="
cargo nextest run --profile ci 2>&1 | tee /app/test_report/test_output.txt
TEST_EXIT=$?

echo "=== Generating coverage report ==="
cargo llvm-cov --lcov --output-path /app/test_report/coverage.lcov 2>/dev/null || true
cargo llvm-cov report --cobertura --output-path /app/test_report/coverage.xml 2>/dev/null || true

echo "=== Running clippy ==="
cargo clippy --all-targets -- -D warnings 2>&1 | tee /app/test_report/clippy_output.txt || true

echo "=== Tests complete ==="
exit $TEST_EXIT
