#!/usr/bin/env bash
# The canonical artifact is the one built on x86_64 Linux (CI, ubuntu-latest): cargo puts the host triple into
# its symbol hashes, so a macOS build is functionally identical but differs byte-for-byte. CI rebuilds on every
# push and fails if the result isn't byte-identical to artifacts/leash_id.wasm.
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
# ME Hub's wasmvm rejects any float instruction at upload ("Float operator detected"); catch it here, not on chain.
if npx wasm-opt --print artifacts/leash_id.wasm | grep -qE "\((f32|f64)\.[a-z]"; then
  echo "error: artifacts/leash_id.wasm contains float instructions; ME Hub will refuse to store it" >&2; exit 1
fi
cosmwasm-check --available-capabilities iterator,staking,stargate,cosmwasm_1_1,cosmwasm_1_2,cosmwasm_1_3,cosmwasm_1_4 artifacts/leash_id.wasm
shasum -a 256 artifacts/leash_id.wasm | tee artifacts/checksums.txt
[ "$(uname -sm)" = "Linux x86_64" ] || echo "note: built on $(uname -sm); the committed artifact comes from x86_64 Linux (CI)" >&2
