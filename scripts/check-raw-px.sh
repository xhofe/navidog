#!/usr/bin/env bash
# Raw-value check: measured Navicat values may only live in navidog-ui tokens/themes.
# Exceptions (runtime geometry, platform boundaries) add `// raw-px: <reason>` on the line or above.
# `.px(...)` is the horizontal padding method and is not checked.
set -euo pipefail
cd "$(dirname "$0")/.."

hits=$(find crates -name '*.rs' ! -path 'crates/navidog-ui/src/tokens.rs' -print0 \
  | xargs -0 awk '
      FNR == 1 { prev = "" }
      /(^|[^.A-Za-z0-9_])(px|rgba?|hsla?)\(/ && $0 !~ /raw-px:/ && prev !~ /raw-px:/ {
        print FILENAME ":" FNR ": " $0
      }
      { prev = $0 }
    ')

if [[ -n "$hits" ]]; then
  echo "Found raw pixels or colors outside token definitions:"
  echo "$hits"
  exit 1
fi
echo "raw-value check passed"
