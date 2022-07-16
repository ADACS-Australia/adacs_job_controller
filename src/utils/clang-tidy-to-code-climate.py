import hashlib
import json
import re
import sys
from pathlib import Path

if __name__ == '__main__':
    input_file = sys.argv[1]
    output_file = sys.argv[2]

    cwd = sys.argv[3]

    catch_re = re.compile("^(\S+):(\d+):(\d+): (\S+): (.+)\[(.+?)(?:,-warnings-as-errors)?\]$")
    file_re = re.compile("^(\S+):(\d+):(\d+): (\S+): (.+)$")
    lines = open(input_file).readlines()

    items = []

    line_number = 0
    while line_number < len(lines):
        result = catch_re.findall(lines[line_number])
        if not result:
            line_number += 1
            continue

        result = list(result[0])

        try:
            result[0] = str(Path(result[0]).relative_to(cwd))
        except ValueError:
            pass

        line_number += 1
        content = []
        while line_number < len(lines):
            next_result = catch_re.findall(lines[line_number])
            if next_result:
                break

            if lines[line_number].startswith("Error while processing") or lines[line_number].startswith("make"):
                break

            content.append(lines[line_number])

            line_number += 1

        result.append(content)
        items.append(result)
        line_number += 1

    for i in range(len(items)):
        item = items[i]

        # Refer to https://github.com/codeclimate/platform/blob/master/spec/analyzers/SPEC.md#issues

        content = None
        if len(item) == 7:
            content = "\n".join(item[6])
            content = f'```\n{content}\n```'

        description = f'{item[3]}: {item[4]} [{item[5]}]'
        fingerprint = hashlib.sha1(f"{item[0]}:{item[1]}:{item[2]}:{description}".encode("utf-8")).hexdigest()

        items[i] = {
            'type': 'issue',
            'check_name': item[5],
            'description': description,
            'content': {
                'body': content
            },
            'categories': ['Bug Risk'],
            'severity': 'major',
            'location': {
                'path': item[0],
                'lines': {
                    'begin': int(item[1]),
                    'end': int(item[1])
                }
            },
            'fingerprint': fingerprint
        }

    with open(output_file, 'w') as f:
        json.dump(items, f)
