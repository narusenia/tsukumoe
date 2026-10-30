#!/usr/bin/env bash
# Dependency rules from docs/specifications/architecture.md:
#  - crates below the UI boundary must not depend on egui*
#  - tsukumoe-core must not depend on wgpu
set -euo pipefail

tree() { cargo tree -p "$1" --edges normal,build --prefix none; }

fail=0
for crate in $(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name' | grep -vE '^tsukumoe-(host|app|ui)$'); do
  if tree "$crate" | grep -E '^(egui|eframe|epaint|ecolor|emath)'; then
    echo "error: $crate must not depend on egui*" >&2
    fail=1
  fi
done
if tree tsukumoe-core | grep -E '^wgpu'; then
  echo "error: tsukumoe-core must not depend on wgpu" >&2
  fail=1
fi
exit "$fail"
