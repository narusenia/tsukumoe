#!/usr/bin/env bash
# Writes <out-dir>/<case>.rgba and <case>.json for every case in tests/parity/cases
# with the native runtime. Usage: tests/parity/export.sh <out-dir>
set -euo pipefail
cd "$(dirname "$0")/../.."
out=${1:?usage: tests/parity/export.sh <out-dir>}
mkdir -p "$out/models"

cargo run -q -p tsukumoe-wasm --example web_fixture -- "$out/models/fixture.tmod"
cargo run -q -p tsukumoe-cli -- generate assets/reference-model/parts.json "$out/models/reference.tmod" >/dev/null
for case in tests/parity/cases/*.json; do
  cargo run -q -p tsukumoe-cli --example parity_export -- "$out/models" "$case" "$out/$(basename "$case" .json)"
done
