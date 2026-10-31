#!/usr/bin/env bash
# Compares the native output of every parity case with tests/parity/golden
# (`tsukumoe parity`, pixels 2/255, draw-list numbers 1e-4). With --update the
# golden files are rewritten from the native output instead. tsukumoe-unity
# keeps a copy of them (its scripts/import-golden.sh).
#   tests/parity/golden.sh <out-dir> [--update]
set -euo pipefail
cd "$(dirname "$0")/../.."
out=${1:?usage: tests/parity/golden.sh <out-dir> [--update]}

bash tests/parity/export.sh "$out"
if [ "${2:-}" = "--update" ]; then
  mkdir -p tests/parity/golden
  rm -f tests/parity/golden/*
  cp "$out"/*.rgba "$out"/*.json tests/parity/golden/
  exit 0
fi

fail=0
for json in tests/parity/cases/*.json; do
  name=$(basename "$json" .json)
  report=$(cargo run -q -p tsukumoe-cli -- parity tests/parity/golden/"$name" "$out/$name" --width 512) || fail=1
  echo "$name: $(jq -c '[.ok, .draw_list.max, .image.max, .image.over]' <<<"$report")"
done
exit $fail
