#!/usr/bin/env bash
# Local single-node ME Hub (built from openmetaearth/me-hub) for the "suspend the ME ID" demo.
# Uses the public dev keys from me-hub/scripts/setup_local.sh: `user` (funded) and `global_dao`
# (= KYC issuer DID 0000000000001). Nothing here touches the public testnet.
#   ./med.sh fund                fund issuer + Leash ID roles
#   ./med.sh approve <role> <did> <level>   issue an ME ID (KYC approve) to a role's address
#   ./med.sh status <did> <active|inactive> global DAO flips a DID's status
#   ./med.sh did <role>          show a role's DID info
set -euo pipefail
D="$(cd "$(dirname "$0")" && pwd)"
MED="${ME_HUB_DIR:-$HOME/workspace/harbor/vendor/me-hub}/build/med"
export HOME="$D/home"
CHAIN=mechain_2401-1
NODE=tcp://localhost:36657
TX=(--chain-id "$CHAIN" --node "$NODE" --keyring-backend test --gas 400000 --fees 20000umec -y -o json)
addr() { awk -v r="$1" '$1==r{print $2}' "$D/keys.txt"; }
pub() { awk -v r="$1" '$1==r{ $1=""; $2=""; sub(/^  /,""); print }' "$D/keys.txt"; }
wait_tx() {
  local h; h=$(jq -r .txhash); local code
  for _ in $(seq 1 20); do
    if out=$("$MED" q tx "$h" --node "$NODE" -o json 2>/dev/null); then
      code=$(echo "$out" | jq -r .code); echo "tx $h code=$code $(echo "$out" | jq -r 'if .code==0 then "" else .raw_log end')"; return 0
    fi
    sleep 1
  done
  echo "tx $h not found"; return 1
}
case "$1" in
  fund)
    G=$("$MED" keys show global_dao -a --keyring-backend test)
    "$MED" tx bank send user "$G" 1000000000umec "${TX[@]}" | wait_tx
    for r in owner agent vendor rogue lowkyc; do "$MED" tx bank send user "$(addr $r)" 500000000umec "${TX[@]}" | wait_tx; done ;;
  approve)
    G=$("$MED" keys show global_dao -a --keyring-backend test)
    "$MED" tx kyc approve "$3" experience_region "$(addr "$2")" "$(pub "$2")" "$4" "https://example.invalid/kyc/$3" "demo-$3" "$G" --from global_dao "${TX[@]}" | wait_tx ;;
  status)
    s=1; [ "$3" = inactive ] && s=0  # CLI takes 1-active, 0-inactive
    "$MED" tx did update-did-status "$2" "$s" --from global_dao "${TX[@]}" | wait_tx ;;
  did)
    curl -s "localhost:1317/metaearth/did/did?address=$(addr "$2")"; echo ;;
  *) echo "usage: $0 fund|approve|status|did"; exit 1 ;;
esac
