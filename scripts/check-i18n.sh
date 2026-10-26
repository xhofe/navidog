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
                keys.setdefault(flat[:-3], {})['en'] = value.strip().strip('"')
            elif flat.endswith('.zh-CN'):
                keys.setdefault(flat[:-6], {})['zh-CN'] = value.strip().strip('"')
    return keys

definitions = {}
by_crate = {}
for path in glob.glob('crates/*/locales/*.yml'):
    crate = path.split('/')[1]
    crate_keys = by_crate.setdefault(crate, set())
    for key, languages in parse(path).items():
        if 'en' not in languages or 'zh-CN' not in languages:
            print(f'{path}: `{key}` must define both en and zh-CN')
            FAIL = True
            continue
        definitions.setdefault(key, []).append((path, languages['en']))
        crate_keys.add(key)

def strip_rust_comments(text):
    """Blanks // and nested /* */ comments while preserving newlines and strings."""
    out = []
    i = 0
    n = len(text)
    while i < n:
        if text.startswith('//', i):
            end = text.find('\n', i)
            if end == -1:
                break
            out.append('\n')
            i = end + 1
        elif text.startswith('/*', i):
            depth = 1
            i += 2
            while i < n and depth:
                if text.startswith('/*', i):
                    depth += 1
                    i += 2
                elif text.startswith('*/', i):
                    depth -= 1
                    i += 2
                else:
                    if text[i] == '\n':
                        out.append('\n')
                    i += 1
        elif text[i] == '"':
            out.append('"')
            i += 1
            while i < n:
                ch = text[i]
                out.append(ch)
                if ch == '\\' and i + 1 < n:
                    out.append(text[i + 1])
                    i += 2
                    continue
                i += 1
                if ch == '"':
                    break
        elif text[i] == 'r' or (text[i] == 'b' and i + 1 < n and text[i + 1] == 'r'):
            start = i
            j = i + 1 if text[i] == 'r' else i + 2
            hashes = 0
            while j < n and text[j] == '#':
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                delim = '"' + '#' * hashes
                out.append(text[start:j + 1])
                i = j + 1
                end = text.find(delim, i)
                if end == -1:
                    out.append(text[i:])
                    break
                out.append(text[i:end + len(delim)])
                i = end + len(delim)
            else:
                out.append(text[i])
                i += 1
        else:
            out.append(text[i])
            i += 1
    return ''.join(out)

# A feature crate's backend only loads its own locale files, so a literal
# `t!(...)` key must be defined there. Cross-crate UI keys resolve through
# explicit helpers, not through another backend. Scan the whole file so
# whitespace and line breaks inside the macro are accepted too.
for source in glob.glob('crates/*/src/**/*.rs', recursive=True):
    crate = source.split('/')[1]
    text = strip_rust_comments(open(source, encoding='utf-8').read())
    for match in re.finditer(r't!\s*\(\s*"([A-Za-z0-9_.-]+)"', text):
        key = match.group(1)
        if key not in by_crate.get(crate, set()):
            line = text.count('\n', 0, match.start()) + 1
            print(f'{source}:{line}: `{key}` is not defined in {crate} locales')
            FAIL = True

for key, entries in sorted(definitions.items()):
    values = {value for _, value in entries}
    if len(entries) > 1 and len(values) > 1:
        print(f'{key}: defined with different English values in ' + ', '.join(source for source, _ in entries))
        FAIL = True

sys.exit(1 if FAIL else 0)
PY
