#!/bin/bash
# Run tests with coverage report
# NOTE: This script must be run from the src/build/ directory

set -e

# Check if we're in the correct directory
if [ ! -f "Boost_Tests_run" ]; then
    echo "Error: This script must be run from the src/build/ directory"
    echo "Usage: cd src/build && ../../scripts/test-coverage.sh"
    exit 1
fi

# Clean up old coverage data files
echo "Cleaning up old coverage data..."
find . -name "*.gcda" -type f -delete

# Run tests with HRF (Human Readable Format) output
echo "Running tests..."
./Boost_Tests_run --logger=HRF,all --color_output=true --report_format=HRF --show_progress=no

# Print separator
printf "\n\n"
echo "================================"
echo "Code Coverage Report"
echo "================================"
printf "\n"

# Generate code coverage summary
gcovr \
  -r .. \
  --object-directory . \
  --gcov-executable "llvm-cov-22 gcov" \
  --print-summary \
  -e ".*/third_party/.*" \
  -e ".*/tests/.*" \
  -e ".*/fixtures/.*" \
  -e ".*_tests\.cpp$" \
  -e ".*test_.*\.cpp$"

echo ""
echo "Coverage report complete!"
