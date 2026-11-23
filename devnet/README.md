# Local ME Hub node (kill-switch demo)

Testnet ME IDs can't be suspended by us, so this runs the same check against a single ME Hub node built from
[openmetaearth/me-hub](https://github.com/openmetaearth/me-hub). We use its public dev keys: `global_dao` is also
KYC issuer `0000000000001`.

```bash
git clone https://github.com/openmetaearth/me-hub && (cd me-hub && make build)   # Go 1.24
export ME_HUB_DIR=$PWD/me-hub
node devnet/keys.mjs            # roles -> devnet/keys.txt (addresses + public keys)
devnet/reset.sh                 # setup_local.sh + our demo ME IDs in the x/did genesis
devnet/start.sh &               # RPC :36657, REST :1317
devnet/med.sh fund
source devnet/env.sh            # point leash.mjs at the local node
node leash.mjs deploy && node scripts/scenario.mjs
bash scripts/killswitch.sh      # -> runs/devnet-killswitch-*.txt
```

ME IDs are put into the `x/did` genesis instead of going through `kyc approve`. On a single-region node,
`approve` fails in the KYC reward step, because it needs a non-experience region with its own validator.
The contract only reads `x/did`, and `x/did` InitGenesis registers address → DID the same way an approval does.
Suspending an ME ID uses the real `MsgUpdateDidStatus` from the global DAO.
