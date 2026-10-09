#!/usr/bin/env bash
# Builds PumboBans for every supported Pumpkin server:
#   dist/PumboBans-26.3.wasm -> Pumpkin 0.2.0+26.3-26.51     (Minecraft 26.3)
#   dist/PumboBans-26.2.wasm -> Pumpkin 0.1.0-dev+26.2-26.45 (Minecraft 26.2)
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
TARGET=wasm32-wasip2
mkdir -p dist

cargo build --release --target "$TARGET" -p pumbo-bans-pumpkin --target-dir "$ROOT/target/bans-263"
cp "$ROOT/target/bans-263/$TARGET/release/pumbo_bans_pumpkin.wasm" dist/PumboBans-26.3.wasm

cargo build --release --target "$TARGET" -p pumbo-bans-pumpkin --no-default-features --features mc262 \
  --target-dir "$ROOT/target/bans-262"
cp "$ROOT/target/bans-262/$TARGET/release/pumbo_bans_pumpkin.wasm" dist/PumboBans-26.2.wasm

ls -l dist/*.wasm
