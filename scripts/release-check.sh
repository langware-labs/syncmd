#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n1)"
python_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/crates/syncmd-py/pyproject.toml" | head -n1)"
npm_version="$(sed -n 's/.*"version": "\(.*\)",/\1/p' "$repo_root/npm/syncmd/package.json" | head -n1)"

echo "Checking version alignment..."
if [[ "$cargo_version" != "$python_version" || "$cargo_version" != "$npm_version" ]]; then
  echo "Version mismatch:"
  echo "  Cargo:  $cargo_version"
  echo "  PyPI:   $python_version"
  echo "  npm:    $npm_version"
  exit 1
fi

echo "Running cargo checks..."
cargo check -p syncmd --manifest-path "$repo_root/Cargo.toml"
cargo check -p syncmd-py --manifest-path "$repo_root/Cargo.toml"

echo "Running npm package checks..."
node --check "$repo_root/npm/syncmd/bin/syncmd.js"
node --check "$repo_root/npm/syncmd/lib/platform.js"
node --check "$repo_root/npm/syncmd/scripts/install.js"
npm pack --dry-run "$repo_root/npm/syncmd" >/dev/null

echo "Release preflight passed for version $cargo_version"
