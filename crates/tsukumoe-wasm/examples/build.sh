#!/usr/bin/env bash
# Builds the stage 1 browser check: build.sh <out-dir> <simd|nosimd>.
# Needs the mise tools (wasm-bindgen, wasm-opt). Writes <out-dir>/<variant>/.
set -euo pipefail
out=$1 variant=$2
cd "$(dirname "$0")/../../.."
flags=""
[ "$variant" = simd ] && flags="-C target-feature=+simd128"
target=target/wasm-$variant
RUSTFLAGS="$flags" CARGO_TARGET_DIR=$target \
  cargo build -p tsukumoe-wasm --target wasm32-unknown-unknown --release
mkdir -p "$out/$variant"
wasm-bindgen --target web --out-dir "$out/$variant" --out-name tsukumoe \
  $target/wasm32-unknown-unknown/release/tsukumoe_wasm.wasm
cp "$out/$variant/tsukumoe_bg.wasm" "$out/$variant/tsukumoe_bg.raw.wasm"
wasm-opt -Oz --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
  --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
  "$out/$variant/tsukumoe_bg.raw.wasm" -o "$out/$variant/tsukumoe_bg.opt.wasm"
for f in raw opt; do
  printf '%s %s: %s bytes, gzip -9 %s\n' "$variant" "$f" \
    "$(wc -c <"$out/$variant/tsukumoe_bg.$f.wasm")" \
    "$(gzip -9 -c "$out/$variant/tsukumoe_bg.$f.wasm" | wc -c)"
done
