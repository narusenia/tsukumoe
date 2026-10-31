#!/usr/bin/env bash
# Runs every parity case natively and on the WASM build, then compares with
# `tsukumoe parity`. Needs `mise run web:build` first.
#   tests/parity/native-vs-wasm.sh <out-dir>
#       draw lists only (WASM in Node)
#   PARITY_IMAGES=1 tests/parity/native-vs-wasm.sh <out-dir>
#       also images: starts tests/parity/web-server.mjs on 127.0.0.1:${PARITY_PORT:-8765}
#       and waits until Chrome has played the cases, so open
#       http://127.0.0.1:${PARITY_PORT:-8765}/parity.html?backend=auto in Chrome
#       (the page posts the pixels and the server then exits).
set -euo pipefail
cd "$(dirname "$0")/../.."
out=${1:?usage: tests/parity/native-vs-wasm.sh <out-dir>}

bash tests/parity/export.sh "$out/native"
node packages/tsukumoe-web/test/parity-export.ts "$out/native/models" tests/parity/cases "$out/wasm"
if [ -n "${PARITY_IMAGES:-}" ]; then
  node tests/parity/web-server.mjs "$out/native/models" "$out/wasm" "${PARITY_PORT:-8765}"
else
  rm -f "$out/native"/*.rgba
fi

fail=0
for json in "$out"/wasm/*.json; do
  name=$(basename "$json" .json)
  report=$(cargo run -q -p tsukumoe-cli -- parity "$out/native/$name" "$out/wasm/$name" --width 512) || fail=1
  echo "$name: $(jq -c '[.ok, .draw_list.max, .image.max, .image.over]' <<<"$report")"
done
exit $fail
