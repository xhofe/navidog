#!/usr/bin/env bash
# Dependency direction (A-02): the domain model, the drivers, the SQL layer and the pure helpers
# must not link GPUI. Only the application-state, session and UI crates may, and
# `navidog-headless` proves model + drivers build and run on their own.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0

for crate in navidog-model navidog-db navidog-sql navidog-util navidog-sql-log navidog-headless; do
    if cargo tree -p "$crate" -e normal --prefix none | grep -qE '^gpui([- ]|$)'; then
        echo "dependency check: $crate links gpui" >&2
        fail=1
    fi
done

for manifest in \
    crates/navidog-model/Cargo.toml \
    crates/navidog-db/Cargo.toml \
    crates/navidog-sql/Cargo.toml \
    crates/navidog-util/Cargo.toml \
    crates/navidog-headless/Cargo.toml; do
    if grep -q 'gpui' "$manifest"; then
        echo "dependency check: $manifest depends on gpui" >&2
        fail=1
    fi
done

# App state moved to `navidog-app-state`; a returning `navidog-core` means a pane started
# reaching for GPUI through the model again.
if [ -d crates/navidog-core ]; then
    echo "dependency check: crates/navidog-core must stay gone (app state lives in navidog-app-state)" >&2
    fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo "dependency check passed"
fi
exit "$fail"
