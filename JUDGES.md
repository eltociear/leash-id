# For judges: a 10-minute tour

**Leash ID** lets an AI agent pay invoices on ME Hub, but only inside a spending leash tied to its owner's live
ME ID. The contract re-reads the ME ID from x/did on every payment, so suspending it stops every agent under it
in the next block. Sellers can require that a verified human answers for an agent before they take its money.

Everything below can be checked without trusting us: each claim links to code, a recorded run, or a chain query.

## Try it (no install)

| What | Where |
|---|---|
| One-page overview | https://eltociear.github.io/leash-id/about.html |
| Check any agent, no wallet | https://eltociear.github.io/leash-id/ (try "an EVM address bound to an ME ID": a real testnet user's sub-account, resolved by the contract from x/did) |
| Owner console (Keplr): approvals, two signers, revoke, limits, fee grants, CSV | https://eltociear.github.io/leash-id/owner.html |
| Demo video | see the top of the [README](README.md) |
| Red-team page (attack a real agent key) | [`sandbox/`](sandbox/README.md): its leash is live on testnet (a 0-MEC key on the owner's fee grant); run it locally with `node scripts/sandbox-dev.mjs` |

## On ME Hub testnet (`mechain_400-1`)

- Contract `me1pggva37n3yr92stjc4vquvsl4dulgm7z34ja24e23t5nck3ezjpqhr958v`, code 36, **no admin** (nobody can swap
  the code). Store tx `F7507A5A…2BCE`, instantiate tx `A73A30C1…A2C3`
  ([REST](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/A73A30C11A72718DAB16C0B337CA4B9A863F5B31E61BEB603DCDA4B36E15A2C3)).
- `npm run verify` (and the `testnet` CI job on every push): the wasm on chain is byte-for-byte the CI build of
  this repo, and the contract has no admin (the CI job is advisory, so a testnet outage can't fail the build).
- **Real x/did records, real transactions** ([`runs/testnet-identity-*.txt`](runs/)): another user's ME ID
  binds an EVM sub-account through `MsgCreateSubAccount`. The contract refuses a false link
  ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/5BADDF9D8BBFDA51EE88A6B4B7C456E92450898BAA931007CF2339AE87FD924B)),
  records the true one ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/C5313B5A7AE17FBC3C682817CD3AF67F6F61F4777E435710E29CC964F97D6A91)),
  and from then on treats that EVM address as the ME ID's.

- **The payment flows, with our owner's real ME ID** (`2611255912050`, KYC level 2, issued by the testnet KYC
  issuer) in [`runs/testnet-payments-2026-11-25T05-22-54Z.txt`](runs/testnet-payments-2026-11-25T05-22-54Z.txt), 20+ transactions anyone can look up:
  - A second real ME ID, at KYC level 1, tries to fund agents and is refused on chain
    ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/25F9DC16734C72D88F16C55883AD129D45415A06DF33315E643B9E463A122088): `has KYC level 1, need 2`).
  - Paid inside the leash; refused: the same invoice spelled differently, over the limit, a vendor with no ME ID, a
    vendor at KYC level 1. Each refusal is a failed tx with its reason.
  - Paid to an **EVM sub-account** of another user's KYC 2 ME ID: the receipt names that ME ID
    ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/0A9620F89D6CDE675DC11935777AE622C04AEB831C719CF2D0F90852EDFD5E60)).
  - Held above the threshold; the agent can't release it; the owner approves. A `multi_send` batch. Revoke → refused,
    resume → paid.
  - An agent key that **never held MEC** pays on the owner's fee grant (the tx's fee granter is the owner:
    [tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/992D931D5B97D1BFB10834BE899EA1BA36CDBFCDCEA5810B622CDD6B1DCDF593)) and can't send anything else.

- **Claude Code as the agent, on testnet** ([`runs/testnet-claude-2026-11-25T05-35-19Z.jsonl`](runs/testnet-claude-2026-11-25T05-35-19Z.jsonl)): paid INV-1001 and MCC-77 (a merchant it
  verified by ME ID), sent INV-1002 for approval, declined the duplicate, the fake CEO and the injected invoice.
- **The obey-everything client, on testnet** ([`runs/testnet-obey-2026-11-25T13-53-33-068Z.txt`](runs/testnet-obey-2026-11-25T13-53-33-068Z.txt)), on the leash Claude had
  just worked: the fake CEO wire, the injected invoice, the limit raise, the withdraw, the one-shot drain and the
  re-spelled duplicates were all refused on chain. One thing was paid: a batch of 5 × 0.014 MEC to the owner's own
  listed vendor, because it fit inside the 0.2 MEC 24-hour limit this owner set. That is the design: below the
  approval threshold, the 24-hour limit is the bound ([T6](THREAT_MODEL.md)). On the local node, with a 0.05 MEC
  limit, the same batch is refused.
- **Agent-to-agent commerce, on testnet** ([`runs/testnet-commerce-2026-11-25T05-37-16Z.txt`](runs/testnet-commerce-2026-11-25T05-37-16Z.txt)): HTTP 402, paid through the leash, replay,
  front-running, an unissued invoice and a seller without an ME ID all refused.
- **The red-team sandbox's leash is live on testnet** (agent `me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan`, holds
  0 MEC, fees on the owner's grant).

The kill switch needs the global DAO's key to suspend an ME ID, so it is shown on a **local ME Hub node built from
`openmetaearth/me-hub`** (see below), with repeats of the obey-everything, Claude and commerce runs. CI repeats the
obey-everything, kill-switch and commerce runs on a fresh node on every push; the Claude run is recorded once and
committed.

## By judging criterion

**Technical implementation (30)**
- Contract: [`contracts/leash-id/src`](contracts/leash-id/src). 33 unit tests, including a random-operation test of
  the accounting invariants (40 seeds × 250 operations: contract balance = sum of owner balances, reserved = sum
  of held payments, with partial two-person approvals mixed in).
- [CI](.github/workflows/ci.yml), three jobs on every push:
  - `contract`: tests, clippy, JS tests, npm audit, a reproducible wasm, a float-instruction guard, and a
    byte-for-byte rebuild of the browser bundle.
  - `devnet`: builds me-hub, runs the attack replay, the kill switch, the sub-account and two-signature flows
    and agent-to-agent commerce on a real node. [`scripts/check-runs.mjs`](scripts/check-runs.mjs) then
    asserts each outcome, and the owner console is driven in Chrome.
  - `testnet`: `verify`.
- Findings from two adversarial reviews are listed, with their fixes, in [CHANGELOG.md](CHANGELOG.md) (0.2.0, 0.4.0).
- Sends hash the tx before broadcasting. An unconfirmed send is reported as UNKNOWN with its hash, never as
  "not sent", so a retry can't double-pay.

**Depth of ME Network integration (25)**
- ME Hub opens three Stargate queries and one custom message to CosmWasm. Leash ID uses every one that carries
  meaning:
  - `/metaearth.did.Query/Did` on every pay, approve, deposit, offer, accept and whois.
  - `/metaearth.wstaking.Query/FixedDeposit`, so an owner can show a bond.
  - The custom `multi_send` message for batches.
  - `Region` is the one left out, and [README](README.md#why-this-is-me-native) explains why.
- The ME ID is the kill switch. The global DAO's `MsgUpdateDidStatus` stops the agent in the next block, and even
  the owner's approval is refused. [`runs/devnet-killswitch-*.txt`](runs/), steps 2-4.
- EVM sub-accounts (x/kyc `MsgCreateSubAccount`) resolve to their ME ID. This is shown on testnet with real
  records (above) and on the node (step 7).
- Payments and fees are in MEC, and receipts store the owner's and the vendor's ME ID numbers.
- The agent key can hold 0 MEC: the owner pays its fees through x/feegrant, contract calls only (step 9).

**Product value (20)**
- The problem, with sources: [README → Why now](README.md). Business email compromise cost close to $2.8B in
  2024, and agents that read invoices inherit it.
- For the owner:
  - limits (per payment, any 24 hours)
  - vendor rules ("listed, or holds an ME ID")
  - approvals above a threshold, with optional two-person approval
  - revoke and resume
  - CSV export
  - Slack alerts (`leash watch`)
- For the seller: `leash-id/vendor` takes payment from AI agents only when a verified human answers for them.
- Cost: about 260k gas per payment ([docs/GAS.md](docs/GAS.md)), and a refusal costs nothing.

**Innovation (15)**
- The chain, not the prompt, is the safety layer. The MCP server even offers owner-only tools in the demo, and the
  contract refuses them. See [`runs/devnet-obey-*.txt`](runs/): a client that obeys every email made 8 attacks
  and none got through.
- HTTP 402 with a verified human on both sides:
  - The buyer checks that the seller holds an ME ID.
  - The seller checks that a live ME ID answers for the buyer.
  - The buyer proves with its key that the payment is its own. Tx hashes are public, so without that proof
    anyone could redeem it.
- [`runs/devnet-commerce-*.txt`](runs/) shows replay, front-running and seller-without-ME-ID attempts all failing.

**User experience (10)**
- A verifier with no wallet, an owner console (Keplr; tested end to end with a mnemonic signer), and a CLI with
  per-command help and exit codes (3 = "outcome unknown, look it up").
- READMEs in [Japanese](README.ja.md) and [Chinese](README.zh-CN.md).
- MCP results Claude can act on: `PAID`, `HELD FOR OWNER APPROVAL`, `REFUSED`, `UNKNOWN`.

## Check it yourself

```bash
git clone https://github.com/eltociear/leash-id && cd leash-id && npm ci
npm run verify                    # testnet: deployed code == this repo's build, no admin
npm run test:js                   # 22 JS tests, no chain needed
node scripts/check-runs.mjs       # the committed run records say what we claim
cat runs/testnet-claude-*.jsonl | jq -r 'select(.type=="result").result'  # Claude's own report (testnet)
```

## Known limits, stated plainly

- Suspending an ME ID needs the global DAO, so the kill switch is shown on a node we run, not on testnet. Every
  other flow is recorded on testnet too, and CI re-runs the obey-everything, kill-switch and commerce runs on a
  fresh node.
- The owner console's Keplr path is written to Keplr's API but was tested with a mnemonic signer (`npm run
  e2e:owner`). Bonds are covered by unit tests against the real x/wstaking response shape. A local node has no
  fixed-deposit configuration to try them against.
- The threat model lists what Leash ID does not protect against: [THREAT_MODEL.md](THREAT_MODEL.md).
