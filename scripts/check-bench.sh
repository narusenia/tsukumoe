#!/usr/bin/env bash
# Fails when a benchmark's median exceeds its ceiling (nanoseconds) in
# crates/tsukumoe-core/benches/budget.json. Run `cargo bench -p tsukumoe-core` first.
set -euo pipefail

budget=crates/tsukumoe-core/benches/budget.json
fail=0
for name in $(jq -r 'keys[]' "$budget"); do
  file="target/criterion/$name/new/estimates.json"
  if [ ! -f "$file" ]; then
    echo "error: no result for '$name' (run cargo bench first)" >&2
    fail=1
    continue
  fi
  median=$(jq '.median.point_estimate | floor' "$file")
  limit=$(jq ".\"$name\"" "$budget")
  if [ "$median" -gt "$limit" ]; then
    echo "FAIL $name: median ${median} ns > ceiling ${limit} ns" >&2
    fail=1
  else
    echo "ok   $name: median ${median} ns <= ${limit} ns"
  fi
done
exit "$fail"
