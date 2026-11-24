#!/usr/bin/env bash
# Every devnet evidence run, from an empty local ME Hub node, with the current artifacts/leash_id.wasm:
#   A) Claude Code as the AP agent over MCP, then the obey-everything client on the same leash
#   B) a fresh node for the ME ID kill switch (scripts/killswitch.sh)
#   C) a fresh node for agent-to-agent commerce over HTTP 402 (scripts/commerce.sh)
# Writes runs/devnet-claude-*.jsonl, runs/devnet-obey-*.txt, runs/devnet-killswitch-*.txt, runs/devnet-commerce-*.txt.
# Needs ME_HUB_DIR (see devnet/README.md) and the `claude` CLI for part A (SKIP_CLAUDE=1 to skip it).
set -euo pipefail
cd "$(dirname "$0")/.."
source devnet/env.sh
export LEASH_INBOX=inbox/devnet.json
# never leave a local node running behind, even when a step fails
trap 'pkill -f "build/med start" || true' EXIT
mkdir -p runs

fresh() {
  devnet/reset.sh > /dev/null
  devnet/start.sh &
  for _ in $(seq 60); do curl -sf localhost:36657/status > /dev/null && break; sleep 1; done
  sleep 3
  devnet/med.sh fund > /dev/null
  node leash.mjs deploy
  node scripts/scenario.mjs
}

# The inbox for this machine's keys: Acme is whatever the local `vendor` key is, and the "verified ME ID
# merchant" is the devnet's DID 0000000000002 holder (me-hub's public dev key).
ACME=$(node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('vendor')))")
sed -e 's/me1ysp96p8g7aagtq05nncptyfkh5kk0zwfl6u9x6/me1p7s6k4ecrm2kl0rs6399k99pyuk322dc78dcxq/g' \
    -e "s/me1j64lzr00fnj8uxwy955au7ac2gzn07hf2qvc3q/$ACME/g" inbox/demo.json > inbox/devnet.json

fresh
if [ -z "${SKIP_CLAUDE:-}" ]; then
  scripts/claude-run.sh devnet-claude
fi
LEASH_OBEY_PREFIX=devnet-obey node mcp/obey-everything.mjs | tail -1

fresh
bash scripts/killswitch.sh | tail -1

fresh
bash scripts/commerce.sh | tail -1
