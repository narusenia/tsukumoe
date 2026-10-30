#!/usr/bin/env bash
# Dependency rules from docs/specifications/architecture.md:
#  - crates below the GPUI boundary must not depend on gpui*
#  - seimei-core must not depend on wgpu
set -euo pipefail

tree() { cargo tree -p "$1" --edges normal,build --prefix none; }

fail=0
for crate in $(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name' | grep -vE '^seimei-(host|app)$'); do
  if tree "$crate" | grep -E '^gpui'; then
    echo "error: $crate must not depend on gpui*" >&2
    fail=1
  fi
done
if tree seimei-core | grep -E '^wgpu'; then
  echo "error: seimei-core must not depend on wgpu" >&2
  fail=1
fi
exit "$fail"
