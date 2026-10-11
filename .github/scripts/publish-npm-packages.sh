#!/usr/bin/env bash
# Usage: publish-npm-packages.sh <version>
# Platform packages go first so the root package never points at a missing optional dependency.
set -euo pipefail

VERSION="$1"
PACKAGES=(
  npm/test-checker-darwin-arm64
  npm/test-checker-darwin-x64
  npm/test-checker-linux-arm64
  npm/test-checker-linux-x64
  npm/test-checker-win32-x64
  npm/test-checker
)

for package in "${PACKAGES[@]}"; do
  name=$(jq -r .name "$package/package.json")
  if [ "$(npm view "$name@$VERSION" version 2>/dev/null)" = "$VERSION" ]; then
    echo "$name@$VERSION is already published; skipping"
    continue
  fi
  (cd "$package" && npm publish --access public)
done
