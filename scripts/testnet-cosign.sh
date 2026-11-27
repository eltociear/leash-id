#!/usr/bin/env bash
# Two signatures on ME Hub testnet, with two real KYC-2 ME IDs (the owner's and the CFO's): a held payment needs
# both; with two signatures required the owner alone can't loosen the leash (it waits 24 h) and the CFO cancels it;
# a withdrawal waits too; the CFO stops the agent at once; then the owner and the CFO together return to one
# signature immediately (approve_change), and the agent resumes. The CFO's key holds no MEC: its fees come from
# the owner's fee grant. Writes runs/testnet-cosign-<ts>.txt.
# Needs the owner, agent, vendor and cfo keys, the agent's leash accepted, and the owner's budget deposited.
set -uo pipefail
cd "$(dirname "$0")/.."
unset ME_CHAIN_ID ME_RPC ME_REST LEASH_DEPLOYMENT
export LEASH_ONCHAIN_REFUSALS=1   # every refusal is a failed tx with a hash, not just a simulation error
mkdir -p runs
OUT="runs/testnet-cosign-$(date -u +%Y-%m-%dT%H-%M-%SZ).txt"
log() { echo "$*" | tee -a "$OUT"; }
step() { log ""; log "## $*"; }
run() { log "\$ $*"; "$@" 2>&1 | tee -a "$OUT"; }
inv() { echo "CS-$(date +%s)-$1"; }
addr() { node -e "import('./lib/chain.mjs').then(async c => console.log(await c.address('$1')))"; }
O=$(addr owner) A=$(addr agent) C=$(addr cfo)

log "Leash ID two-signature run on ME Hub testnet (mechain_400-1)"
log "contract $(jq -r .contract deployment.json)"

step "1. two real ME IDs at KYC level 2: the owner's and the CFO's (the CFO key holds no MEC)"
run node leash.mjs identity owner
run node leash.mjs identity cfo
run node leash.mjs gas-grant "$C" --limit 0.05 --days 75      # the CFO's fees come from the owner's grant

step "2. the owner adds the CFO as co-approver: a held payment needs both signatures"
run node leash.mjs approvers cfo --threshold 2
run node leash.mjs pay vendor 0.018 "$(inv g)" --as agent     # over approve_above: held
HID=$(grep -oE 'payment #[0-9]+' "$OUT" | tail -1 | grep -oE '[0-9]+$')
run node leash.mjs approve "$HID"                             # the owner's signature: 1 of 2, nothing paid yet
run node leash.mjs approve "$HID" --as agent                  # the agent can't sign
run node leash.mjs approve "$HID" --as cfo --fee-granter owner # the CFO's signature: 2 of 2, paid

step "3. with two signatures required, the owner key alone can't loosen the leash"
run node leash.mjs policy agent --approve-above none          # dropping the approval line waits 24 h
run node leash.mjs whois agent                                 # still approval above the line; the change is waiting
run node leash.mjs changes
run node leash.mjs cancel-change "policy:$A" --owner "$O" --as cfo --fee-granter owner   # the CFO says no
run node leash.mjs withdraw 0.01                              # a withdrawal waits too: where a thief would send the budget
run node leash.mjs changes
run node leash.mjs cancel-change withdraw --owner "$O" --as cfo --fee-granter owner
run node leash.mjs revoke agent --as cfo --fee-granter owner  # and any co-approver can stop the agent at once
run node leash.mjs pay vendor 0.001 "$(inv r)" --as agent     # refused: revoked

step "4. back to one signature, by both of them: approve_change applies it at once"
run node leash.mjs approvers --threshold 1                    # the owner proposes: it waits
run node leash.mjs approve-change approvers --owner "$O" --as cfo --fee-granter owner   # the CFO agrees: applied now
run node leash.mjs approvers
run node leash.mjs resume agent
run node leash.mjs pay vendor 0.001 "$(inv b)" --as agent     # paid again

log ""
log "done -> $OUT"
