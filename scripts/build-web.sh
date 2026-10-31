#!/usr/bin/env bash
# Builds the npm package packages/tsukumoe-web: tsukumoe-wasm for wasm32 (with
# and without SIMD), wasm-bindgen glue, wasm-opt, then the TypeScript.
# Needs the mise tools (wasm-bindgen, wasm-opt, node). Run it as
# `mise run web:build`.
set -euo pipefail
cd "$(dirname "$0")/.."
pkg=packages/tsukumoe-web
rm -rf "$pkg/src/wasm" "$pkg/dist"

for variant in simd nosimd; do
  flags=""
  [ "$variant" = simd ] && flags="-C target-feature=+simd128"
  # A separate target directory: RUSTFLAGS differ, so the builds must not share one.
  RUSTFLAGS="$flags" CARGO_TARGET_DIR="target/web-$variant" \
    cargo build -p tsukumoe-wasm --profile wasm --target wasm32-unknown-unknown
  out="$pkg/src/wasm/$variant"
  wasm-bindgen --target web --out-dir "$out" --out-name tsukumoe \
    "target/web-$variant/wasm32-unknown-unknown/wasm/tsukumoe_wasm.wasm"
  wasm-opt -Oz --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$out/tsukumoe_bg.wasm" -o "$out/tsukumoe_bg.wasm.opt"
  mv "$out/tsukumoe_bg.wasm.opt" "$out/tsukumoe_bg.wasm"
done

cd "$pkg"
npm ci --no-audit --no-fund
npm run build:ts
cd ../..
bash scripts/check-wasm-size.sh
