#!/usr/bin/env bash
# The distributed wasm must stay within 1 MiB gzipped (M3-04 acceptance).
set -euo pipefail
cd "$(dirname "$0")/.."
limit=1048576
fail=0
for f in packages/tsukumoe-web/dist/wasm/*/tsukumoe_bg.wasm; do
  size=$(gzip -9 -c "$f" | wc -c | tr -d ' ')
  echo "$f: $size bytes gzipped (limit $limit)"
  if [ "$size" -gt "$limit" ]; then
    echo "error: $f is over the size budget" >&2
    fail=1
  fi
done
exit "$fail"
