#!/usr/bin/env bash
# Agent-to-agent commerce on a local ME Hub node: a paid API (examples/paid-api) sells only to agents with a
# verified human behind them; our agent buys through its leash, and only from sellers that hold an ME ID.
# Run after the demo leash is in place (scripts/devnet-runs.sh does it). Writes runs/devnet-commerce-<ts>.txt.
set -uo pipefail
cd "$(dirname "$0")/.."
source devnet/env.sh
mkdir -p runs
OUT="runs/devnet-commerce-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | sed "s#$PWD/##g" | tee -a "$OUT"; }
api() { curl -s -w '\n(HTTP %{http_code})\n' "$@"; }

PORT=18790 LEASH_PAY_TO_ROLE=shop node examples/paid-api/server.mjs 2>/dev/null & A=$!
PORT=18791 LEASH_PAY_TO_ROLE=rogue node examples/paid-api/server.mjs 2>/dev/null & B=$!
trap 'kill $A $B 2>/dev/null' EXIT
for _ in $(seq 100); do curl -s localhost:18790/ >/dev/null && curl -s localhost:18791/ >/dev/null && break; sleep 0.3; done

log "Agent-to-agent commerce on a LOCAL ME Hub node ($ME_CHAIN_ID)"
log "seller: a paid API, payments to the shop (ME ID 1000000000103); buyer: our leashed agent over MCP"

step "1. the API asks for payment (HTTP 402) with its Leash ID terms"
run api localhost:18790/chain-stats

step "2. the agent's MCP tool pay_for_api: checks the seller's ME ID, pays through the leash, gets the data"
run node scripts/commerce-client.mjs http://localhost:18790/chain-stats 0.002
TX=$(grep -o '"tx": "[0-9A-F]*"' "$OUT" | tail -1 | cut -d'"' -f4)

INV=$(grep -oE 'invoice=API-[0-9A-Z-]+' "$OUT" | tail -1 | cut -d= -f2)
proof() { node scripts/commerce-proof.mjs "$@"; }

step "3. the same payment can't buy a second answer (even with the agent's own proof)"
run api -H "X-Leash-Payment: $TX" -H "X-Leash-Proof: $(proof agent "$TX" "$INV")" localhost:18790/chain-stats

step "4. tx hashes are public: someone watching the chain tries to redeem a fresh payment first"
INV3=$(curl -s localhost:18790/chain-stats | jq -r .leash_id.invoice)
run node leash.mjs pay shop 0.001 "$INV3" --as agent
TX3=$(grep -oE 'txs/[0-9A-F]{64}' "$OUT" | tail -1 | cut -d/ -f2)
run api -H "X-Leash-Payment: $TX3" -H "X-Leash-Proof: $(proof rogue "$TX3" "$INV3")" localhost:18790/chain-stats
log "# the agent that paid, with its own signature"
run api -H "X-Leash-Payment: $TX3" -H "X-Leash-Proof: $(proof agent "$TX3" "$INV3")" localhost:18790/chain-stats

step "5. a made-up payment, and a real leash payment that isn't for an invoice the API issued"
FAKE=$(openssl rand -hex 32 | tr a-f A-F)
run api -H "X-Leash-Payment: $FAKE" -H "X-Leash-Proof: $(proof agent "$FAKE" x)" localhost:18790/chain-stats
run node leash.mjs pay shop 0.001 "NOT-ISSUED-$(date +%s)" --as agent
TX2=$(grep -oE 'txs/[0-9A-F]{64}' "$OUT" | tail -1 | cut -d/ -f2)
INV2=$(grep -oE 'invoice=NOT-ISSUED-[0-9]+' "$OUT" | tail -1 | cut -d= -f2)
run api -H "X-Leash-Payment: $TX2" -H "X-Leash-Proof: $(proof agent "$TX2" "$INV2")" localhost:18790/chain-stats

step "6. a seller without an ME ID: the agent's tool won't pay it, and the contract wouldn't either"
run node scripts/commerce-client.mjs http://localhost:18791/chain-stats 0.002
run node leash.mjs pay rogue 0.001 "DIRECT-$(date +%s)" --as agent

step "7. too expensive: the agent's own cap"
run node scripts/commerce-client.mjs http://localhost:18790/chain-stats 0.0005

log ""
log "done -> $OUT"
