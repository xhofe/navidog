#!/usr/bin/env bash
# Fail the macOS bundle when cargo-bundle omitted the Finder icon.
# Globs in package.metadata.bundle are relative to the working directory;
# a miss used to succeed with no CFBundleIconFile.
set -euo pipefail

if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
  echo "Usage: $0 path/to/Navidog.app" >&2
  exit 2
fi

app="$1"
icon="$app/Contents/Resources/navidog.icns"
if [ ! -f "$icon" ]; then
  echo "missing $icon" >&2
  exit 1
fi

name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$app/Contents/Info.plist")"
if [ "$name" != "navidog.icns" ]; then
  echo "CFBundleIconFile is '${name}', expected navidog.icns" >&2
  exit 1
fi
