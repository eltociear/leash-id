#!/usr/bin/env bash
# Reproducible build: Rust 1.81 (no bulk-memory/reference-types, which wasmvm 1.4 on ME Hub rejects),
# then binaryen wasm-opt, then cosmwasm-check against ME Hub's capability set (wasmd 0.43 AllCapabilities).
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/.cargo/env"
# Remap local paths (cargo registry, this checkout) so the wasm doesn't depend on who built it or where:
# the same source gives the same sha256 on any machine, including CI.
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
REMAP="--remap-path-prefix=$CARGO_HOME=/cargo --remap-path-prefix=$PWD=/src"
( cd contracts/leash-id && RUSTFLAGS="-C link-arg=-s $REMAP" cargo +1.81.0 build -q --release --lib --target wasm32-unknown-unknown )
mkdir -p artifacts
npx wasm-opt -Oz --signext-lowering contracts/leash-id/target/wasm32-unknown-unknown/release/leash_id.wasm -o artifacts/leash_id.wasm
cosmwasm-check --available-capabilities iterator,staking,stargate,cosmwasm_1_1,cosmwasm_1_2,cosmwasm_1_3,cosmwasm_1_4 artifacts/leash_id.wasm
shasum -a 256 artifacts/leash_id.wasm | tee artifacts/checksums.txt
