#!/usr/bin/env sh
set -eu
cargo build --target wasm32-unknown-unknown --release --no-default-features --features wasm
mkdir -p public/pkg
wasm-bindgen \
  --target web \
  --no-typescript \
  --out-dir public/pkg \
  --out-name qrforge_wasm \
  target/wasm32-unknown-unknown/release/qrforge.wasm
