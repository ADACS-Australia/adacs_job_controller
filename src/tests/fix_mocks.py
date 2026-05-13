"""Fix mock closures that now need Box::pin(async {}) after send_message became async."""
import re
import sys

files = sys.argv[1:]

for filepath in files:
    with open(filepath, 'r') as f:
        lines = f.readlines()

    new_lines = []
    i = 0
    fixed = 0
    while i < len(lines):
        line = lines[i]
        new_lines.append(line)

        if 'expect_send_message()' in line and 'returning(' in line and 'Box::pin' not in line:
            brace_count = line.count('{') - line.count('}')

            if brace_count > 0:
                # Find end of closure
                j = i + 1
                while j < len(lines):
                    brace_count += lines[j].count('{') - lines[j].count('}')
                    if brace_count <= 0:
                        stripped = lines[j].strip()
                        if stripped == '});' or stripped == '})();':
                            indent = len(lines[j]) - len(lines[j].lstrip())
                            new_lines.append(' ' * indent + 'Box::pin(async {})\n')
                            fixed += 1
                        break
                    j += 1
        i += 1

    if fixed > 0:
        with open(filepath, 'w') as f:
            f.writelines(new_lines)
        print(f"Fixed {fixed} closures in {filepath}")
    else:
        print(f"No changes in {filepath}")
