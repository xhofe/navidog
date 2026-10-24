#!/usr/bin/env bash
# Fails when production code contains Chinese text, or when two locale files
# define the same key with different English values. Test data, examples and
# the language autonym are excluded.
set -euo pipefail
cd "$(dirname "$0")/.."

python3 - <<'PY'
import glob, os, re, sys

CJK = re.compile(r'[\u4e00-\u9fff]')
FAIL = False
for path in glob.glob('crates/*/src/**/*.rs', recursive=True):
    if '/tests/' in path or path.endswith('_tests.rs') or path.endswith('/tests.rs'):
        continue
    text = open(path, encoding='utf-8').read()
    for marker in ('#[cfg(test)]', '\nmod tests'):
        at = text.find(marker)
        if at != -1:
            text = text[:at]
    for number, line in enumerate(text.splitlines(), 1):
        if '简体中文' in line:
            continue
        if CJK.search(line):
            print(f'{path}:{number}: Chinese text in production code: {line.strip()}')
            FAIL = True

def parse(path):
    keys = {}
    stack = []
    for line in open(path, encoding='utf-8'):
        if not line.strip() or line.strip().startswith('#'):
            continue
        indent = len(line) - len(line.lstrip(' '))
        match = re.match(r'^([A-Za-z0-9_.-]+):\s*(.*)$', line.strip())
        if not match:
            continue
        key, value = match.group(1), match.group(2)
        while stack and stack[-1][0] >= indent:
            stack.pop()
        stack.append((indent, key))
        if value:
            flat = '.'.join(k for _, k in stack)
            if flat.endswith('.en'):
                keys[flat[:-3]] = (path, value.strip().strip('"'))
    return keys

definitions = {}
for path in glob.glob('crates/*/locales/*.yml'):
    for key, (source, value) in parse(path).items():
        definitions.setdefault(key, []).append((source, value))

for key, entries in sorted(definitions.items()):
    values = {value for _, value in entries}
    if len(entries) > 1 and len(values) > 1:
        print(f'{key}: defined with different English values in ' + ', '.join(source for source, _ in entries))
        FAIL = True

sys.exit(1 if FAIL else 0)
PY
