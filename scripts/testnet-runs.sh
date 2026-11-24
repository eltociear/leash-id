#!/usr/bin/env bash
# What can run on ME Hub testnet without our own ME ID: the sub-account resolution against REAL x/did records
# (MsgCreateSubAccount bindings other users made), identity lookups, and the contract's refusals.
# Writes runs/testnet-identity-<ts>.txt. Needs the `owner` key with a little testnet MEC for fees.
set -uo pipefail
cd "$(dirname "$0")/.."
unset ME_CHAIN_ID ME_RPC ME_REST LEASH_DEPLOYMENT
export LEASH_ONCHAIN_REFUSALS=1   # record refusals as failed txs anyone can look up
mkdir -p runs
OUT="runs/testnet-identity-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$OUT"; }
R=https://beta-hub-1317.explorer-testnet.me

# pick a real binding the contract hasn't been told about yet: an active, KYC level 2 ME ID whose EVM
# sub-account x/did lists
DID=""
while read -r d m sb; do
  if node leash.mjs identity "$sb" | grep -q "^ME ID    none"; then DID=$d MAIN=$m SUB=$sb; break; fi
done < <(curl -s "$R/metaearth/did/did_infos?pagination.limit=200" | jq -r '.infos[] | select(.sub_account != "" and .status == "DID_STATUS_ACTIVE" and .kycLevel == "KYC_LEVEL_TWO") | "\(.did) \(.address) \(.sub_account)"')
[ -n "$DID" ] || { echo "no unlinked sub-account left in the first 200 ME IDs"; exit 1; }
OTHER=$(curl -s "$R/metaearth/did/did_infos?pagination.limit=200" | jq -r --arg m "$MAIN" '[.infos[] | select(.sub_account != "" and .address != $m)][0].address')

log "Leash ID on ME Hub TESTNET (mechain_400-1), contract $(jq -r .contract deployment.json) (code $(jq -r .codeId deployment.json), no admin)"
log "a real x/did record: ME ID $DID, main account $MAIN, EVM sub-account $SUB"

step "1. x/did, straight from REST: the main account holds the ME ID and lists the sub-account"
log "\$ curl $R/metaearth/did/did?address=$MAIN"
curl -s "$R/metaearth/did/did?address=$MAIN" | jq -c '.info | {did, address, status, kycLevel, sub_account}' | tee -a "$OUT"
log "\$ curl $R/metaearth/did/did?address=$SUB"
curl -s "$R/metaearth/did/did?address=$SUB" | jq -c . | tee -a "$OUT"

step "2. the contract, before anyone records the link: the EVM address has no ME ID of its own"
run node leash.mjs identity "$SUB"

step "3. a false link (another ME ID's main account) is refused on chain"
run node leash.mjs link-sub "$SUB" "$OTHER" --as owner

step "4. the true link: anyone may record it; the contract checks x/did itself"
run node leash.mjs link-sub "$SUB" "$MAIN" --as owner

step "5. the contract now resolves the EVM address to its ME ID, read live from x/did"
run node leash.mjs identity "$SUB"
run node leash.mjs meid "$MAIN"

step "6. the contract's own checks, read-only: whois on an address nobody leashed"
run node leash.mjs whois "$SUB"

log ""
log "done -> $OUT"
