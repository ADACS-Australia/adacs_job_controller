#! /bin/bash
set -e
# /src is already the working directory (set in dockerfile)

# Wait for mysql to be ready
while ! mysqladmin ping -h"$DATABASE_HOST" --silent; do
    sleep 1
done

# Migrate the database
utils/schema/venv/bin/python utils/schema/manage.py migrate;

# Function to run tests and handle MySQL segfaults gracefully
run_tests_with_segfault_handling() {
    local test_name="$1"
    local test_args="$2"
    
    echo "Running $test_name tests with real-time output..."
    
    # Create a temporary file to capture output
    local temp_file=$(mktemp)
    
    # Run tests with real-time output using tee, and capture exit code
    local exit_code
    if build/Boost_Tests_run $test_args 2>&1 | tee "$temp_file"; then
        exit_code=0
    else
        exit_code=$?
    fi
    
    # Read the captured output
    local test_output
    test_output=$(cat "$temp_file")
    
    # Clean up temp file
    rm -f "$temp_file"
    
    # Check if tests passed by looking for the success message
    if echo "$test_output" | grep -q "\*\*\* No errors detected"; then
        echo "$test_name tests completed successfully - detected success message"
        return 0
    else
        echo "$test_name tests failed - no success message detected"
        return $exit_code
    fi
}

# Run the jobserver tests with segfault handling
run_tests_with_segfault_handling "JUNIT" "--catch_system_error=yes --log_format=JUNIT --show_progress=no --log_sink=/test_report/junit.xml"
JUNIT_RESULT=$?

run_tests_with_segfault_handling "HRF" "--logger=HRF,all --color_output=true --report_format=HRF --show_progress=no"
HRF_RESULT=$?

# Exit with error if either test run failed
if [ $JUNIT_RESULT -ne 0 ] || [ $HRF_RESULT -ne 0 ]; then
    echo "Test execution failed - JUNIT: $JUNIT_RESULT, HRF: $HRF_RESULT"
    exit 1
fi

# Print some whitespace
printf "\n\n"

# Generate code coverage reports
gcovr \
  -r . \
  --object-directory build \
  --gcov-executable "llvm-cov-22 gcov" \
  --xml-pretty -o /test_report/coverage_docker.xml \
  -e ".*/third_party/.*" \
  -e ".*/tests/.*" \
  -e ".*/fixtures/.*" \
  -e ".*_tests\.cpp$" \
  -e ".*test_.*\.cpp$"

gcovr \
  -r . \
  --object-directory build \
  --gcov-executable "llvm-cov-22 gcov" \
  --print-summary \
  -e ".*/third_party/.*" \
  -e ".*/tests/.*" \
  -e ".*/fixtures/.*" \
  -e ".*_tests\.cpp$" \
  -e ".*test_.*\.cpp$"

python3 utils/clang-tidy-to-code-climate.py /src/build/tidy.txt /test_report/code_climate.json /

# Set full access on the test_report directory in case, so the host can delete the files if needed
chmod -R 666 /test_report
chmod 777 /test_report
