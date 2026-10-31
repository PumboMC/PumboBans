#!/usr/bin/env bash
# Builds PumboBans for every supported Pumpkin server:
#   dist/PumboBans-26.3.wasm  -> Pumpkin 0.2.1+26.3-26.51     (Minecraft 26.3)
#   dist/PumboBans-0.2.0.wasm -> Pumpkin 0.2.0+26.3-26.51     (Minecraft 26.3)
#   dist/PumboBans-26.2.wasm  -> Pumpkin 0.1.0-dev+26.2-26.45 (Minecraft 26.2)
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
TARGET=wasm32-wasip2
mkdir -p dist

cargo build --release --target "$TARGET" -p pumbo-bans-pumpkin --target-dir "$ROOT/target/bans-263"
cp "$ROOT/target/bans-263/$TARGET/release/pumbo_bans_pumpkin.wasm" dist/PumboBans-26.3.wasm

# Pumpkin 0.2.0 and 0.2.1 reject each other's builds, and Cargo cannot hold
# both API versions at once, so the lockfile is repinned to 0.2.0 for this
# build and put back afterwards.
mkdir -p "$ROOT/target"
cp "$ROOT/Cargo.lock" "$ROOT/target/Cargo.lock.keep"
trap 'cp "$ROOT/target/Cargo.lock.keep" "$ROOT/Cargo.lock"' EXIT
cargo update -q -p pumpkin-plugin-api@0.2.1 --precise 0.2.0+26.3-26.51
PUMBO_PUMPKIN=0.2.0 cargo build --release --target "$TARGET" -p pumbo-bans-pumpkin --locked \
  --target-dir "$ROOT/target/bans-263-020"
cp "$ROOT/target/bans-263-020/$TARGET/release/pumbo_bans_pumpkin.wasm" dist/PumboBans-0.2.0.wasm
cp "$ROOT/target/Cargo.lock.keep" "$ROOT/Cargo.lock"

cargo build --release --target "$TARGET" -p pumbo-bans-pumpkin --no-default-features --features mc262 \
  --target-dir "$ROOT/target/bans-262"
cp "$ROOT/target/bans-262/$TARGET/release/pumbo_bans_pumpkin.wasm" dist/PumboBans-26.2.wasm

# Each file names the api it was built against in its panic locations; a file
# with the wrong api would not load on its Pumpkin.
for f in 26.3:0.2.1+26.3-26.51 0.2.0:0.2.0+26.3-26.51 26.2:0.1.0-dev+26.2-26.45; do
  if ! grep -qaF "pumpkin-plugin-api-${f#*:}/" "dist/PumboBans-${f%%:*}.wasm"; then
    echo "dist/PumboBans-${f%%:*}.wasm is not built against pumpkin-plugin-api ${f#*:}" >&2
    exit 1
  fi
done
ls -l dist/*.wasm
