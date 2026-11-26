#!/usr/bin/env bash
# A real bond on ME Hub TESTNET: the owner locks MEC in an x/wstaking fixed deposit and points the contract at it.
# Every step is a transaction or a REST read anyone can repeat. Writes runs/testnet-bond-<ts>.txt.
# Needs: owner (ME ID, KYC 2, in a region with a fixed-deposit term; on testnet only me_earth: 1 day, ≥ 0.01 MEC).
set -uo pipefail
cd "$(dirname "$0")/.."
unset ME_CHAIN_ID ME_RPC ME_REST LEASH_DEPLOYMENT
export LEASH_ONCHAIN_REFUSALS=1
mkdir -p runs
OUT="runs/testnet-bond-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$OUT"; }
get() { log "\$ curl $R$1"; curl -s --retry 5 --retry-all-errors --retry-delay 2 "$R$1" | jq -c "${2:-.}" | tee -a "$OUT"; }
addr() { node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('$1')))"; }
R=https://beta-hub-1317.explorer-testnet.me
OWNER=$(addr owner)
DID=$(curl -s --retry 5 --retry-all-errors "$R/metaearth/did/did?address=$OWNER" | jq -r .info.did)
SANDBOX=${SANDBOX:-me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan}

log "Leash ID on ME Hub TESTNET (mechain_400-1), contract $(jq -r .contract deployment.json) (code $(jq -r .codeId deployment.json), no admin)"
log "owner $OWNER, ME ID $DID"

step "1. ME Hub's own records: the owner's KYC region, and the fixed-deposit terms each region offers"
get "/metaearth/kyc/KYC?did=$DID" '{did: .kyc.did, region: (.kyc.data | @base64d)}'
get "/metaearth/wstaking/fixed_deposit_cfg" '[.RegionFixedDepositCfgs[] | select(.RegionFixedDepositCfg | length > 0)]'

step "2. before: the contract sees no bond"
run node leash.mjs identity owner

step "3. someone else's fixed deposit can't be shown as ours: the contract reads x/wstaking and refuses"
OTHER=$(curl -s --retry 5 --retry-all-errors "$R/metaearth/wstaking/fixed_deposit?pagination.limit=50" | jq -r --arg me "$OWNER" '[.FixedDeposit[]? | select(.account != $me)][0].id')
log "# deposit #$OTHER belongs to another account"
get "/metaearth/wstaking/fixed_deposit/$OTHER" '.FixedDeposit | {id, account, principal}'
run node leash.mjs bond "$OTHER"

step "4. lock 0.02 MEC for the region's term (MsgDoFixedDeposit) and show it as the bond (set_bond), in one command"
run node leash.mjs lock 0.02
DEP=$(grep -oE 'fixed deposit #[0-9]+' "$OUT" | tail -1 | grep -oE '[0-9]+$')

step "5. after: x/wstaking holds the deposit, and the contract reads it live"
get "/metaearth/wstaking/fixed_deposit/$DEP" '.FixedDeposit | {id, account, principal, end_time, term}'
run node leash.mjs identity owner
run node leash.mjs whois "$SANDBOX"

log ""
log "# A deposit stops counting the moment it unlocks (end_time above): the contract checks end_time on every read,"
log "# so the bond drops out by itself; nothing has to be cleared."
log "done -> $OUT"
