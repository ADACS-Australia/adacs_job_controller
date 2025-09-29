#!/bin/bash

# Script to check if code is properly formatted with clang-format
# Exits with non-zero code if any files would be modified by clang-format

set -e

echo "Checking clang-format compliance..."

# Find all C++ source files (excluding build and third_party directories)
FILES=$(find src -name "*.cpp" -o -name "*.cppm" -o -name "*.ixx" -o -name "*.h" | \
        grep -v "build/" | grep -v "third_party/")

# Check if any files would be modified by clang-format
if clang-format --dry-run --Werror $FILES >/dev/null 2>&1; then
    echo "✅ All code is properly formatted with clang-format!"
    exit 0
else
    echo "❌ Code is not properly formatted with clang-format!"
    echo "The following files need formatting:"
    clang-format --dry-run $FILES 2>&1 | grep -E "(^src/|would be reformatted)" | grep -v "clang-format:"
    echo ""
    echo "Run the following command to fix formatting:"
    echo "find src -name \"*.cpp\" -o -name \"*.cppm\" -o -name \"*.ixx\" -o -name \"*.h\" | grep -v \"build/\" | grep -v \"third_party/\" | xargs clang-format -i"
    exit 1
fi