#!/usr/bin/env bash
# Dependency rules from docs/specifications/architecture.md:
#  - crates below the UI boundary must not depend on egui*
#  - tsukumoe-core must not depend on wgpu
#  - tsukumoe-runtime depends on tsukumoe-core only (no tsukumoe-format, no
#    other workspace crate) and uses only its evaluation side, never the
#    editing model or its commands
#  - tsukumoe-ffi depends on tsukumoe-runtime and tsukumoe-core only (dev
#    dependencies such as cbindgen and cc are not looked at)
#  - tsukumoe-wasm depends on tsukumoe-runtime, tsukumoe-gpu and tsukumoe-core only
#  - tsukumoe-psd depends on tsukumoe-core only
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
if tree tsukumoe-runtime | grep -E '^(tsukumoe-)' | grep -vE '^tsukumoe-(runtime|core)( |$)'; then
  echo "error: tsukumoe-runtime must depend on tsukumoe-core only" >&2
  fail=1
fi
# The editing side of tsukumoe-core (model, commands, history, tables, rig
# types) is off limits to the runtime's sources (its tests may build models).
if grep -rnE 'tsukumoe_core::(model|command|history|validate|table|arena|rig|keyform|driver|physics|expr)\b|tsukumoe_format' crates/tsukumoe-runtime/src; then
  echo "error: tsukumoe-runtime sources must not use the editing side of tsukumoe-core or tsukumoe-format" >&2
  fail=1
fi
if tree tsukumoe-ffi | grep -E '^(tsukumoe-)' | grep -vE '^tsukumoe-(ffi|runtime|core)( |$)'; then
  echo "error: tsukumoe-ffi must depend on tsukumoe-runtime and tsukumoe-core only" >&2
  fail=1
fi
if tree tsukumoe-wasm | grep -E '^(tsukumoe-)' | grep -vE '^tsukumoe-(wasm|runtime|gpu|core)( |$)'; then
  echo "error: tsukumoe-wasm must depend on tsukumoe-runtime, tsukumoe-gpu and tsukumoe-core only" >&2
  fail=1
fi
if tree tsukumoe-psd | grep -E '^(tsukumoe-)' | grep -vE '^tsukumoe-(psd|core)( |$)'; then
  echo "error: tsukumoe-psd must depend on tsukumoe-core only" >&2
  fail=1
fi
# `unsafe` lives in tsukumoe-ffi only (wasm-bindgen's generated code is not
# written by us). The one exception is the allocation
# counter in the evaluator's tests (it proves a frame does not allocate).
if grep -rnE '\bunsafe[[:space:]]+(impl|fn|trait|extern|\{)' crates --include='*.rs' | grep -vE '^crates/tsukumoe-ffi/|^crates/tsukumoe-core/src/eval\.rs:'; then
  echo "error: unsafe code outside tsukumoe-ffi" >&2
  fail=1
fi
exit "$fail"
