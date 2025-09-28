#!/usr/bin/env python3
"""
Convert jobserver_schema.h to a C++20 module (.ixx file)
"""

import os
import sys
import re
from pathlib import Path

def convert_header_to_module(header_file_path, output_file_path):
    """
    Convert a C++ header file to a C++20 module file.
    
    Args:
        header_file_path: Path to the input .h file
        output_file_path: Path to the output .ixx file
    """
    
    with open(header_file_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # Remove the NOLINTBEGIN comment at the start
    content = re.sub(r'^// NOLINTBEGIN\n', '', content)
    
    # Remove the NOLINTEND comment at the end
    content = re.sub(r'\n// NOLINTEND$', '', content)
    
    # Remove header guards
    content = re.sub(r'#ifndef SCHEMA_JOBSERVER_SCHEMA_H\n#define SCHEMA_JOBSERVER_SCHEMA_H\n', '', content)
    content = re.sub(r'\n#endif$', '', content)
    
    # Remove #pragma once
    content = re.sub(r'#pragma once\n', '', content)
    
    # Remove duplicate includes that will be in the global module fragment
    content = re.sub(r'#include <sqlpp11/table\.h>\n', '', content)
    content = re.sub(r'#include <sqlpp11/data_types\.h>\n', '', content)
    content = re.sub(r'#include <sqlpp11/char_sequence\.h>\n', '', content)
    
    # Add export keywords to make types visible
    # Export namespace declarations
    content = re.sub(r'^namespace schema$', 'export namespace schema', content, flags=re.MULTILINE)
    
    # Don't add export to structs inside exported namespace - they're already exported
    
    # Add module declaration at the top
    module_content = """module;
#include <sqlpp11/table.h>
#include <sqlpp11/data_types.h>
#include <sqlpp11/char_sequence.h>

export module jobserver_schema;

"""
    
    # Add the converted content
    module_content += content
    
    # Write the module file
    with open(output_file_path, 'w', encoding='utf-8') as f:
        f.write(module_content)
    
    print(f"Converted {header_file_path} to {output_file_path}")

def main():
    if len(sys.argv) != 3:
        print("Usage: python convert_schema_to_module.py <input.h> <output.ixx>")
        sys.exit(1)
    
    input_file = sys.argv[1]
    output_file = sys.argv[2]
    
    if not os.path.exists(input_file):
        print(f"Error: Input file {input_file} does not exist")
        sys.exit(1)
    
    # Create output directory if it doesn't exist
    output_dir = os.path.dirname(output_file)
    if output_dir:
        os.makedirs(output_dir, exist_ok=True)
    
    convert_header_to_module(input_file, output_file)

if __name__ == "__main__":
    main()
