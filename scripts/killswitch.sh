#!/usr/bin/env bash
# The ME ID kill switch, on a local ME Hub node (testnet DIDs cannot be suspended by us):
#   agent pays -> global DAO suspends the owner's ME ID -> the agent's next payment fails in the
#   contract -> ME ID re-activated -> the same payment goes through. Plus the KYC-level gate and a
#   batch settled through ME Hub's multi_send. Writes runs/devnet-killswitch-<ts>.txt.
# Needs a running local node with the contract deployed and the demo leash in place:
#   devnet/reset.sh, devnet/start.sh &, devnet/med.sh fund, node leash.mjs deploy, node scripts/scenario.mjs
#   (scripts/devnet-runs.sh does all of that).
set -uo pipefail
cd "$(dirname "$0")/.."
DEV=devnet   # relative, so the run record has no local paths
source "$DEV/env.sh"
export LEASH_ONCHAIN_REFUSALS=1   # every refusal is a failed tx with a hash, not just a simulation error
OWNER_DID=1000000000101
mkdir -p runs
OUT="runs/devnet-killswitch-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$OUT"; }
inv() { echo "KS-$(date +%s)-$1"; }

log "Leash ID kill-switch run on a LOCAL ME Hub node ($ME_CHAIN_ID, me-hub built from source)"
log "contract $(jq -r .contract "$LEASH_DEPLOYMENT")"

step "1. owner holds a live ME ID; the agent pays an allow-listed vendor"
run "$DEV/med.sh" did owner
run node leash.mjs pay vendor 0.01 "$(inv a)" --as agent
run node leash.mjs pay vendor 0.018 "$(inv p)" --as agent   # over approve_above: held as payment #2

step "2. the global DAO suspends the owner's ME ID (x/did MsgUpdateDidStatus)"
run "$DEV/med.sh" status "$OWNER_DID" inactive
run "$DEV/med.sh" did owner

step "3. next block: the same agent, same vendor, same limits"
run node leash.mjs whois agent
run node leash.mjs check vendor 0.01 "$(inv b)"
B=$(inv b)
run node -e "import('./lib/tx.mjs').then(async ({act}) => { const {address} = await import('./lib/chain.mjs'); const r = await act('agent', {pay:{vendor: await address('vendor'), amount:'1000000', invoice:'$B', memo:null}}); console.log(r.status.toUpperCase() + ': ' + r.reason + '  ' + r.link); })"
log "# the suspended owner tries to push the held payment out"
run node -e "import('./lib/tx.mjs').then(async ({act}) => { const r = await act('owner', {approve:{id:2}}); console.log(r.status.toUpperCase() + ': ' + r.reason + '  ' + r.link); })"

step "4. ME ID re-activated: the payment goes through again, and the owner can approve #2"
run "$DEV/med.sh" status "$OWNER_DID" active
run node leash.mjs pay vendor 0.01 "$B" --as agent
run node leash.mjs approve 2

step "5. KYC level gate: an owner with only real-person verification (KYC_LEVEL_ONE)"
run "$DEV/med.sh" did lowkyc
run node -e "import('./lib/tx.mjs').then(async ({act}) => { const r = await act('lowkyc', {deposit:{}}, [{denom:'umec', amount:'1000000'}]); console.log(r.status.toUpperCase() + ': ' + r.reason + '  ' + r.link); })"

step "6. a batch settled through ME Hub's custom multi_send message (to Acme + DID 0000000000002, a verified ME ID holder not on the list)"
run node leash.mjs batch "vendor:0.004:$(inv c)" "me1p7s6k4ecrm2kl0rs6399k99pyuk322dc78dcxq:0.003:$(inv d)" --as agent
run node leash.mjs whois agent

step "7. an EVM-form sub-account: a shop (ME ID 1000000000103) takes payment at the EVM address bound to its ME ID"
M=$(node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('merchant')))")
run node leash.mjs identity "$M"
run node leash.mjs pay "$M" 0.002 "$(inv e)" --as agent     # on its own the EVM address has no ME ID: refused
run node leash.mjs link-sub "$M" shop --as vendor   # anyone may record it; the contract checks x/did
run node leash.mjs identity "$M"
run node leash.mjs pay "$M" 0.002 "$(inv e)" --as agent     # now paid, with vendor_did 1000000000103 on the receipt

step "8. two signatures: the owner adds a co-approver (ME ID 1000000000104); a held payment needs both"
run node leash.mjs policy agent --daily 0.2                    # only the 24h limit changes; everything else stays
run node leash.mjs approvers cfo --threshold 2
run node leash.mjs pay vendor 0.018 "$(inv g)" --as agent     # held
HID=$(grep -oE 'pay \(pending\) payment #[0-9]+' "$OUT" | tail -1 | grep -oE '[0-9]+$')
run node leash.mjs approve "$HID"                             # the owner's signature: 1 of 2, nothing paid yet
run node leash.mjs approve "$HID" --as agent                  # the agent can't sign
run node leash.mjs approve "$HID" --as cfo                    # the co-approver's signature: 2 of 2, paid

step "9. an agent key that holds no MEC at all: the owner's fee grant pays its contract calls, and nothing else"
A0=$(node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('agent0')))")
O=$(node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('owner')))")
run node leash.mjs gas-grant "$A0" --limit 0.01 --days 30          # x/feegrant: MsgExecuteContract only, capped
run node leash.mjs offer "$A0" --per-tx 0.02 --daily 0.05 --vendors vendor --label zero-balance
run node leash.mjs accept "$O" --as agent0 --fee-granter owner
run node leash.mjs pay vendor 0.002 "$(inv h)" --as agent0 --fee-granter owner
run node leash.mjs whois "$A0"
log "# the same key tries a plain transfer: the grant doesn't cover it, and the key owns nothing"
run node -e "import('./lib/tx.mjs').then(async ({send}) => { const {address} = await import('./lib/chain.mjs'); const r = await send('agent0', await address('rogue'), 1000, { granter: '$O' }); console.log(r.status.toUpperCase() + ': ' + r.reason); })"

log ""
log "done -> $OUT"
