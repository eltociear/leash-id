#!/usr/bin/env bash
# The payment flows on ME Hub TESTNET, with our owner's real ME ID (issued by the testnet KYC issuer, KYC level 2)
# and a second real ME ID at KYC level 1. Every step is a transaction anyone can look up; refusals are recorded
# as failed transactions too (LEASH_ONCHAIN_REFUSALS=1). Writes runs/testnet-payments-<ts>.txt.
# Needs: owner (ME ID, KYC 2, ~0.5 MEC), agent (a little MEC for fees), lowkyc (ME ID, KYC 1, a little MEC),
# agent0 (no MEC at all), vendor (receives).
set -uo pipefail
cd "$(dirname "$0")/.."
unset ME_CHAIN_ID ME_RPC ME_REST LEASH_DEPLOYMENT
export LEASH_ONCHAIN_REFUSALS=1
mkdir -p runs
OUT="runs/testnet-payments-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$OUT"; }
inv() { echo "TN-$(date +%s)-$1"; }
addr() { node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('$1')))"; }
R=https://beta-hub-1317.explorer-testnet.me
OWNER=$(addr owner) AGENT=$(addr agent) AGENT0=$(addr agent0) LOW=$(addr lowkyc) VENDOR=$(addr vendor) ROGUE=$(addr rogue)
# an EVM sub-account bound by x/did to another user's ME ID (KYC 2), linked in runs/testnet-identity-*.txt
SUB=${SUB:-me1lfr0r096aj9fgf3k2sapawn0chpnw7gxdw0vcn}
# its main account, as the contract resolves it from x/did (the link is recorded by scripts/testnet-runs.sh / link-sub)
SUBMAIN=$(node leash.mjs identity "$SUB" | grep -oE 'via its main account me1[0-9a-z]+' | awk '{print $NF}')
[ -n "$SUBMAIN" ] || { echo "$SUB isn't linked to its main account in this contract: run leash link-sub first"; exit 1; }

log "Leash ID on ME Hub TESTNET (mechain_400-1), contract $(jq -r .contract deployment.json) (code $(jq -r .codeId deployment.json), no admin)"
log "owner $OWNER · agent $AGENT · zero-balance agent $AGENT0 · vendor $VENDOR"

# start clean when this contract already carries leashes from an earlier run of this script
for who in agent agent0; do
  if node leash.mjs whois "$who" > /dev/null 2>&1; then
    node leash.mjs leave --as "$who" $([ "$who" = agent0 ] && echo "--fee-granter owner") > /dev/null 2>&1 || true
  fi
done

step "1. the owner's ME ID, straight from x/did (issued by the testnet KYC issuer)"
log "\$ curl $R/metaearth/did/did?address=$OWNER"
curl -s --retry 5 --retry-all-errors --retry-delay 2 "$R/metaearth/did/did?address=$OWNER" | jq -c '.info | {did, address, status, kycLevel}' | tee -a "$OUT"
run node leash.mjs meid "$OWNER"

step "2. a KYC level 1 ME ID (real-person check only) can't fund agents: the contract reads x/did and refuses"
log "\$ curl $R/metaearth/did/did?address=$LOW"
curl -s --retry 5 --retry-all-errors --retry-delay 2 "$R/metaearth/did/did?address=$LOW" | jq -c '.info | {did, address, status, kycLevel}' | tee -a "$OUT"
run node -e "import('./lib/tx.mjs').then(async ({act}) => { const r = await act('lowkyc', {deposit:{}}, [{denom:'umec', amount:'100000'}]); console.log(r.status.toUpperCase() + ': ' + r.reason + '  ' + r.link); })"

step "3. the owner funds a budget and offers a leash; the agent key accepts it"
run node leash.mjs deposit 0.3
run node leash.mjs offer "$AGENT" --per-tx 0.02 --daily 0.06 --approve-above 0.015 --vendors "$VENDOR" --any-verified-vendor yes --label ap-clerk
run node leash.mjs accept "$OWNER" --as agent
run node leash.mjs whois "$AGENT"

step "4. the agent pays inside the leash"
I1=$(inv a)
run node leash.mjs pay vendor 0.01 "$I1" --as agent
log "# the same invoice again, spelled differently: already handled"
run node leash.mjs pay vendor 0.01 "$(echo "$I1" | tr 'A-Z' 'a-z' | tr '-' '_')" --as agent
log "# over the per-payment limit"
run node leash.mjs pay vendor 0.03 "$(inv b)" --as agent
log "# a vendor that is neither listed nor holds an ME ID"
run node leash.mjs pay "$ROGUE" 0.001 "$(inv c)" --as agent
log "# a vendor that holds an ME ID, but only KYC level 1"
run node leash.mjs pay "$LOW" 0.001 "$(inv d)" --as agent
log "# an EVM sub-account x/did binds to a KYC level 2 ME ID: a verified vendor through its main account"
IE=$(inv e)
run node leash.mjs pay "$SUB" 0.001 "$IE" --as agent
SUBID=$(grep -oE 'pay \(paid\) payment #[0-9]+' "$OUT" | tail -1 | grep -oE '[0-9]+$')
log "# the same invoice to that ME ID's main account: one vendor, two addresses, still one invoice (contract 0.5)"
run node leash.mjs pay "$SUBMAIN" 0.001 "$IE" --as agent

step "5. above the approval threshold: held until the owner signs"
run node leash.mjs pay vendor 0.018 "$(inv f)" --as agent
HID=$(grep -oE 'pay \(pending\) payment #[0-9]+' "$OUT" | tail -1 | grep -oE '[0-9]+$')
run node leash.mjs pending
run node leash.mjs approve "$HID" --as agent     # the agent can't release its own payment
run node leash.mjs approve "$HID"

step "5b. two-person approval needs two people: two addresses of one ME ID can't be two co-approvers (contract 0.5)"
log "# $SUBMAIN holds an ME ID; $SUB is its EVM sub-account, so x/did and the contract see one person"
run node leash.mjs approvers "$SUBMAIN,$SUB" --threshold 3
run node leash.mjs approvers --owner "$OWNER"

step "6. a batch through ME Hub's multi_send"
run node leash.mjs batch "vendor:0.002:$(inv g)" "vendor:0.003:$(inv h)" --as agent

step "7. the owner revokes the agent: the next payment is refused; resume lifts it"
run node leash.mjs revoke "$AGENT"
run node leash.mjs pay vendor 0.001 "$(inv i)" --as agent
run node leash.mjs resume "$AGENT"
run node leash.mjs pay vendor 0.001 "$(inv i)" --as agent

step "8. an agent key that holds no MEC at all: the owner's fee grant pays its contract calls, and nothing else"
log "\$ curl $R/cosmos/bank/v1beta1/balances/$AGENT0"
curl -s --retry 5 --retry-all-errors --retry-delay 2 "$R/cosmos/bank/v1beta1/balances/$AGENT0" | jq -c .balances | tee -a "$OUT"
run node leash.mjs gas-grant "$AGENT0" --limit 0.01 --days 30
run node leash.mjs offer "$AGENT0" --per-tx 0.01 --daily 0.02 --vendors "$VENDOR" --label zero-balance
run node leash.mjs accept "$OWNER" --as agent0 --fee-granter owner
run node leash.mjs pay vendor 0.002 "$(inv j)" --as agent0 --fee-granter owner
run node leash.mjs whois "$AGENT0"
log "# the same key tries a plain transfer: the grant doesn't cover it, and the key owns nothing"
run node -e "import('./lib/tx.mjs').then(async ({send}) => { const r = await send('agent0', '$ROGUE', 1000, { granter: '$OWNER' }); console.log(r.status.toUpperCase() + ': ' + r.reason); })"

step "9. the record: the owner's payment history, each receipt carrying both ME IDs"
run node leash.mjs history --owner "$OWNER"
log "# receipt #$SUBID, read from the contract: the vendor was paid at an EVM sub-account, and the receipt names its ME ID"
run node -e "import('./lib/chain.mjs').then(async ({q}) => { const p = await q({payment:{id:$SUBID}}); console.log(JSON.stringify({id: p.id, status: p.status, amount: p.amount, vendor: p.vendor, vendor_did: p.vendor_did, owner_did: p.owner_did, invoice: p.invoice})); })"

log ""
log "done -> $OUT"
