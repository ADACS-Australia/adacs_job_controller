"""Fix mock closures: insert Box::pin before the closing line.""" 
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
        
        # Check if this line starts a multi-line expect_send_message returning closure
        if 'expect_send_message()' in line and 'returning(' in line and 'Box::pin' not in line:
            brace_count = line.count('{') - line.count('}')
            
            if brace_count > 0:
                # Find the closing line
                close_idx = None
                bc = brace_count
                for j in range(i + 1, len(lines)):
                    bc += lines[j].count('{') - lines[j].count('}')
                    if bc <= 0:
                        close_idx = j
                        break
                
                if close_idx is not None:
                    stripped = lines[close_idx].strip()
                    if stripped.startswith('});') or stripped == '});':
                        # Insert Box::pin BEFORE the closing line
                        indent = len(lines[close_idx]) - len(lines[close_idx].lstrip())
                        box_line = ' ' * indent + 'Box::pin(async {})\n'
                        # We're at line i. Add everything from i to close_idx-1, then box_line, then close_idx line
                        for k in range(i, close_idx):
                            new_lines.append(lines[k])
                        new_lines.append(box_line)
                        new_lines.append(lines[close_idx])
                        i = close_idx + 1
                        fixed += 1
                        continue
        
        new_lines.append(line)
        i += 1

    if fixed > 0:
        with open(filepath, 'w') as f:
            f.writelines(new_lines)
        print(f"Fixed {fixed} closures in {filepath}")
    else:
        print(f"No changes in {filepath}")
