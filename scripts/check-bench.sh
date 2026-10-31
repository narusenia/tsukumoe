#!/usr/bin/env bash
# Run `cargo bench --workspace` first. Fails when a benchmark's median
#  - exceeds its ceiling (nanoseconds) in scripts/bench-budget.json, times
#    BENCH_CEILING_SCALE (default 1; the ceilings are for the reference
#    machine, so CI runners set a larger one and rely on the baseline), or
#  - is more than 10% (BENCH_THRESHOLD_PERCENT) above the same benchmark in
#    the baseline file named by BENCH_BASELINE (a results file of an earlier
#    run, normally main's; skipped when unset or missing).
# The medians of this run go to target/bench-results.json, to be kept as the
# next baseline. `gpu_*` and `frame_*` benchmarks need a hardware GPU and are
# skipped when they have no result.
set -euo pipefail

budget=scripts/bench-budget.json
results=target/bench-results.json
threshold=${BENCH_THRESHOLD_PERCENT:-10}
baseline=${BENCH_BASELINE:-}
scale=${BENCH_CEILING_SCALE:-1}
[ -n "$baseline" ] && [ ! -f "$baseline" ] && { echo "note: no baseline at $baseline, skipping the comparison"; baseline=; }

fail=0
echo '{}' > "$results"
for name in $(jq -r 'keys[]' "$budget"); do
  file="target/criterion/$name/new/estimates.json"
  if [ ! -f "$file" ]; then
    case "$name" in
      gpu_* | frame_*) echo "skip $name: no result (needs a hardware GPU)"; continue ;;
    esac
    echo "error: no result for '$name' (run cargo bench first)" >&2
    fail=1
    continue
  fi
  median=$(jq '.median.point_estimate | floor' "$file")
  limit=$(jq ".\"$name\" * $scale | floor" "$budget")
  jq --arg n "$name" --argjson m "$median" '.[$n] = $m' "$results" > "$results.tmp" && mv "$results.tmp" "$results"
  if [ "$median" -gt "$limit" ]; then
    echo "FAIL $name: median ${median} ns > ceiling ${limit} ns" >&2
    fail=1
  else
    echo "ok   $name: median ${median} ns <= ${limit} ns"
  fi
  if [ -n "$baseline" ]; then
    base=$(jq ".\"$name\" // empty" "$baseline")
    if [ -n "$base" ]; then
      if [ $((median * 100)) -gt $((base * (100 + threshold))) ]; then
        echo "FAIL $name: median ${median} ns is more than ${threshold}% above the baseline ${base} ns" >&2
        fail=1
      else
        echo "     $name: baseline ${base} ns"
      fi
    fi
  fi
done
exit "$fail"
