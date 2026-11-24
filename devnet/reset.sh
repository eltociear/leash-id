#!/usr/bin/env bash
# Rebuild the local single-node ME Hub from scratch:
#   me-hub/scripts/setup_local.sh (public dev keys) + our demo ME IDs in the did genesis
#   + no EIP-1559 base fee + 1 s blocks. Then start it with ./start.sh and fund with ./med.sh fund.
# Why genesis ME IDs: on a single-region devnet, `kyc approve` fails in the KYC reward step
# (it needs a non-experience region with its own validator). The contract only reads x/did, and
# x/did InitGenesis registers address -> DID exactly like an approval does. The `shop` ME ID (1000000000103)
# also gets an EVM-form sub-account (the `merchant` key), as MsgCreateSubAccount would bind it.
set -euo pipefail
D="$(cd "$(dirname "$0")" && pwd)"
HUB="${ME_HUB_DIR:-$HOME/workspace/harbor/vendor/me-hub}"
pkill -f "$HUB/build/med start" 2>/dev/null || true
sleep 1
export HOME="$D/home" PATH="$HUB/build:$PATH"
rm -rf "$HOME/.mechain"
cd "$HUB"
printf 'n\ny\n' | sh scripts/setup_local.sh > "$D/setup.log" 2>&1
G="$HOME/.mechain/config/genesis.json"
info() { # role did level
  local addr pub; addr=$(awk -v r="$1" '$1==r{print $2}' "$D/keys.txt"); pub=$(awk -v r="$1" '$1==r{ $1=""; $2=""; sub(/^  /,""); print }' "$D/keys.txt")
  jq -n --arg did "$2" --arg a "$addr" --arg p "$pub" --arg l "$3" '{did:$did,address:$a,pubkey:$p,status:"DID_STATUS_ACTIVE",regionId:"",kycLevel:$l,sub_account:""}'
}
jq --argjson o "$(info owner 1000000000101 KYC_LEVEL_TWO)" --argjson l "$(info lowkyc 1000000000102 KYC_LEVEL_ONE)" \
   --argjson s "$(info shop 1000000000103 KYC_LEVEL_TWO)" --arg sub "$(awk '$1=="merchant"{print $2}' "$D/keys.txt")" \
   '.app_state.did.infos += [$o, $l, ($s | .sub_account = $sub)]
    | .app_state.feemarket.params.no_base_fee = true | .app_state.feemarket.params.base_fee = "0"' "$G" > "$G.tmp"
mv "$G.tmp" "$G"
C="$HOME/.mechain/config/config.toml"
sed 's/^timeout_commit = .*/timeout_commit = "1s"/' "$C" > "$C.tmp" && mv "$C.tmp" "$C"   # portable (GNU and BSD sed)
med validate-genesis
echo "reset done; start with $D/start.sh"
