#!/usr/bin/env bash
# Builds PumboBans for PumboProx into dist/pumbo-bans.wasm: one file to drop
# into the proxy's plugins/. The manifest (pumbo-bans.yml), the default
# config (../pumbo-bans-pumpkin/assets/config.yml) and the messages
# (assets/lang/) are built in; the proxy writes plugins/pumbo-bans/config.yml
# and lang/*.yml at the first start.
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
TARGET=wasm32-wasip2
mkdir -p dist
cargo build --release --target "$TARGET" -p pumbo-bans-prox --target-dir "$ROOT/target/bans-prox"
cp "$ROOT/target/bans-prox/$TARGET/release/pumbo_bans_prox.wasm" dist/pumbo-bans.wasm
ls -l dist/pumbo-bans.wasm
