#!/usr/bin/env bash
# Fails when production code branches on a dialect's product id. Feature code must ask the
# `Dialect` capability methods (or `DriverInfo` flags) instead, so adding a database cannot
# silently fall through an `else` branch. Tests may branch: they name a product on purpose.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
# `.id()` on a dialect, compared or matched in production code.
while IFS=: read -r file line _; do
    case "$file" in
        */tests/*|*tests.rs|*_tests.rs) continue ;;
    esac
    echo "dialect branch: $file:$line" >&2
    fail=1
done < <(grep -rnE '(dialect|d)\.id\(\)[[:space:]]*(==|!=)|match[[:space:]]+[A-Za-z_.]*\.id\(\)' \
    --include='*.rs' crates/ || true)

if [ "$fail" -ne 0 ]; then
    echo "dialect branch check failed: replace the product comparison with a capability method" >&2
fi
exit "$fail"
