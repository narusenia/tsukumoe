#!/usr/bin/env bash
# Runs every parity case natively and on the WASM build in Node, then compares
# the draw lists with `tsukumoe parity` (images need Chrome and are not part of
# this). Needs `mise run web:build` first. Usage: tests/parity/native-vs-wasm.sh <out-dir>
set -euo pipefail
cd "$(dirname "$0")/../.."
out=${1:?usage: tests/parity/native-vs-wasm.sh <out-dir>}

bash tests/parity/export.sh "$out/native"
rm -f "$out/native"/*.rgba # the WASM side writes no images here
node packages/tsukumoe-web/test/parity-export.ts "$out/native/models" tests/parity/cases "$out/wasm"

fail=0
for json in "$out"/wasm/*.json; do
  name=$(basename "$json" .json)
  report=$(cargo run -q -p tsukumoe-cli -- parity "$out/native/$name" "$out/wasm/$name") || fail=1
  echo "$name: $(jq -c '[.ok, .draw_list.max, (.draw_list.mismatches | length)]' <<<"$report")"
done
exit $fail
