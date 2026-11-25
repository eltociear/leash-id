#!/usr/bin/env bash
# The ME ID kill switch on ME Hub TESTNET, ready for the day someone with the power to suspend an ME ID does it.
# Only ME Hub's global DAO (x/did MsgUpdateDidStatus) or the KYC issuer (x/kyc MsgRemove) can change an ME ID's
# status, so we can't trigger it ourselves. This script prepares the evidence, then waits:
#   1. now: the agent pays, and a larger payment is held for the owner
#   2. it watches x/did until the owner's ME ID stops being active (any block, any reason)
#   3. at once: whois, the agent's next payment, and the owner's approval of the held payment, all refused on chain
#   4. if the ME ID comes back (MsgUpdateDidStatus to active, or a new KYC approval): the same payment goes through
# Writes runs/testnet-killswitch-<ts>.txt. Run it in a terminal you leave open:
#   bash scripts/testnet-killswitch.sh            (WAIT_HOURS=48 by default)
set -uo pipefail
cd "$(dirname "$0")/.."
unset ME_CHAIN_ID ME_RPC ME_REST LEASH_DEPLOYMENT
export LEASH_ONCHAIN_REFUSALS=1
mkdir -p runs
OUT="runs/testnet-killswitch-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$OUT"; }
inv() { echo "KS-$(date +%s)-$1"; }
R=https://beta-hub-1317.explorer-testnet.me
OWNER=$(node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('owner')))")
status() { curl -s --retry 5 --retry-all-errors --retry-delay 2 "$R/metaearth/did/did?address=$OWNER" | jq -r '.info.status + " " + .info.kycLevel'; }
WAIT_S=$(( ${WAIT_HOURS:-48} * 3600 ))

log "Leash ID kill switch on ME Hub TESTNET (mechain_400-1), contract $(jq -r .contract deployment.json) (code $(jq -r .codeId deployment.json), no admin)"
log "owner $OWNER"

step "1. the owner's ME ID is live; the agent pays, and a larger payment waits for the owner"
log "\$ curl $R/metaearth/did/did?address=$OWNER"
curl -s --retry 5 --retry-all-errors "$R/metaearth/did/did?address=$OWNER" | jq -c '.info | {did, status, kycLevel}' | tee -a "$OUT"
run node leash.mjs pay vendor 0.002 "$(inv a)" --as agent
run node leash.mjs pay vendor 0.016 "$(inv held)" --as agent
HID=$(grep -oE 'pay \(pending\) payment #[0-9]+' "$OUT" | tail -1 | grep -oE '[0-9]+$')
[ -n "$HID" ] || { log "no held payment (is the leash's approve_above under 0.016 MEC?)"; exit 1; }

step "2. waiting for the owner's ME ID to be suspended (checked every 6 s, up to $((WAIT_S / 3600)) h)"
START=$(date +%s)
while [[ "$(status)" == DID_STATUS_ACTIVE* ]]; do
  (( $(date +%s) - START > WAIT_S )) && { log "still active after $((WAIT_S / 3600)) h; stopping"; exit 2; }
  sleep 6
done
H=$(curl -s "$R/cosmos/base/tendermint/v1beta1/blocks/latest" | jq -r .block.header.height)
log "$(date -u +%H:%M:%SZ) x/did now says: $(status) (seen at height $H)"
log "\$ curl $R/metaearth/did/did?address=$OWNER"
curl -s --retry 5 --retry-all-errors "$R/metaearth/did/did?address=$OWNER" | jq -c '.info | {did, status, kycLevel}' | tee -a "$OUT"

step "3. right away: the agent can't pay, and even the owner can't release the held payment"
run node leash.mjs whois agent
B=$(inv b)
run node leash.mjs pay vendor 0.002 "$B" --as agent
run node leash.mjs approve "$HID"

step "4. waiting for the ME ID to come back (up to $((WAIT_S / 3600)) h)"
START=$(date +%s)
until [[ "$(status)" == DID_STATUS_ACTIVE* ]]; do
  (( $(date +%s) - START > WAIT_S )) && { log "not re-activated within $((WAIT_S / 3600)) h; the record ends here"; log "done -> $OUT"; exit 0; }
  sleep 6
done
log "$(date -u +%H:%M:%SZ) x/did now says: $(status)"
run node leash.mjs pay vendor 0.002 "$B" --as agent
run node leash.mjs whois agent

log ""
log "done -> $OUT"
