#!/usr/bin/env bash
# Fails when a benchmark's median exceeds its ceiling (nanoseconds) in
# scripts/bench-budget.json. Run `cargo bench --workspace` first.
set -euo pipefail

budget=scripts/bench-budget.json
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
