#!/usr/bin/env bash
# Fails when production code branches on a dialect's product id. Feature code must ask the
# `Dialect` capability methods (or a `DialectFeatures` field) instead, so adding a database
# cannot silently fall through an `else` branch. Tests may branch: they name a product on
# purpose.
set -euo pipefail
cd "$(dirname "$0")/.."

# A receiver that is a dialect (`dialect`, `self.dialect`, `t.dialect`, … or the short `d`),
# followed by `.id()`. One pattern covers both comparison orders, `matches!` and `match`.
# The short `d` needs an identifier boundary on comparisons, so `thread.id()` is not
# treated as `d.id()`. `matches!` and `match` stay unanchored; their samples already
# require `matches!(` or `match `.
receiver='([A-Za-z_][A-Za-z0-9_]*\.)*([Dd]ialect|d)\.id\(\)'
boundary='(^|[^A-Za-z0-9_.])'
pattern="(${boundary}${receiver}[[:space:]]*(==|!=))|((==|!=)[[:space:]]*${boundary}${receiver})|(matches!\([[:space:]]*${receiver})|(match[[:space:]]+${receiver})"

# Prove the pattern still catches every shape before trusting a clean run.
samples=(
    'if d.id() == "mysql" {}'
    'if dialect.id() != "sqlite" {}'
    'if "postgres" == dialect.id() {}'
    'if matches!(self.dialect.id(), "sqlite") {}'
    'match dialect.id() { "mysql" => {}, _ => {} }'
    'let x = t.dialect.id() == "postgres";'
)
for sample in "${samples[@]}"; do
    if ! grep -qE "$pattern" <<<"$sample"; then
        echo "self-test: the pattern no longer catches: $sample" >&2
        exit 2
    fi
done

negative_samples=(
    'if thread.id() == other {}'
)
for sample in "${negative_samples[@]}"; do
    if grep -qE "$pattern" <<<"$sample"; then
        echo "self-test: the pattern incorrectly catches: $sample" >&2
        exit 2
    fi
done

fail=0
while IFS=: read -r file line _; do
    case "$file" in
        */tests/*|*tests.rs|*_tests.rs) continue ;;
    esac
    echo "dialect branch: $file:$line" >&2
    fail=1
done < <(grep -rnE "$pattern" --include='*.rs' crates/ || true)

if [ "$fail" -ne 0 ]; then
    echo "dialect branch check failed: replace the product comparison with a capability method" >&2
fi
exit "$fail"
