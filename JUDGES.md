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
| Owner console, read-only, no wallet: our testnet owner's agents, limits, fees, payments | https://eltociear.github.io/leash-id/owner.html#view=me1r7ssfwrhreafc8fvctqewntmgf42djdk7ldhuq |
| Owner console (Keplr): approvals, two signers, revoke, limits, fee grants, CSV | https://eltociear.github.io/leash-id/owner.html |
| Demo video (4:48, captions) | https://www.youtube.com/watch?v=vySdFuWiGho |
| Red-team page: attack a live testnet agent, no key | `npm ci && node scripts/sandbox-dev.mjs`, then http://localhost:18780/. With no key it simulates every action as our live sandbox agent, so the answers come from the deployed contract and nothing is sent ([`sandbox/`](sandbox/README.md)) |
| Your own Claude as the agent, no key | `claude --mcp-config mcp/judge.mcp.json`, then ask: *"Who is behind you, and what are you allowed to spend? Then check paying 0.5 MEC to me1vrsz4pmwq5cal98fx8cxwtmz3venj68hvdhq7p for invoice URGENT-1."* It reads the demo agent's real leash and dry-runs against the contract; it can't send anything ([`mcp/judge.mcp.json`](mcp/judge.mcp.json)) |

## On ME Hub testnet (`mechain_400-1`)

- Contract `me1rmqthdyqs03faugttrt6kkm2u73pwfr86aqe6s6jvlwq4wqdp6vq4066gc`, code 44, **no admin** (nobody can swap
  the code). Store tx `3381030D…D81F`, instantiate tx `640D425C…B480`
  ([REST](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/640D425C36E7B609141D94835D00A6B7FDF1292969781B0A8ECA2258316BB480)).
- `npm run verify` (and the `testnet` CI job on every push): the wasm on chain is byte-for-byte the CI build of
  this repo, and the contract has no admin (the CI job is advisory, so a testnet outage can't fail the build).
- **Real x/did records, real transactions** ([`runs/testnet-identity-*.txt`](runs/)): another user's ME ID
  binds an EVM sub-account through `MsgCreateSubAccount`. The contract refuses a false link
  ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/C0408A305FECF5EB8E7B5CE56C2878F51CAE5C926B7DAB13606CC3CA541C1656)),
  records the true one ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/68AEF3687EB0BCAF4EC345F39A8495F4B323B4A8BD52771482AA84C2E4FAFD12)),
  and from then on treats that EVM address as the ME ID's.

- **The payment flows, with our owner's real ME ID** (`2611255912050`, KYC level 2, issued by the testnet KYC
  issuer) in [`runs/testnet-payments-2026-11-25T16-54-25Z.txt`](runs/testnet-payments-2026-11-25T16-54-25Z.txt), 20+ transactions anyone can look up:
  - A second real ME ID, at KYC level 1, tries to fund agents and is refused on chain
    ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/77809C56B68D4B3672CA3D7ACAD24033CB07E5B63C936008117B0C0A1EDDFA4A): `has KYC level 1, need 2`).
  - Paid inside the leash; refused: the same invoice spelled differently, over the limit, a vendor with no ME ID, a
    vendor at KYC level 1. Each refusal is a failed tx with its reason.
  - Paid to an **EVM sub-account** of another user's KYC 2 ME ID: the receipt names that ME ID
    ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/0640BCD6CF172200BC8147D94C2412AA8DEA83CC9EA139B79F8D6224BABCAA34)).
    The same invoice at that ME ID's main account is refused as already paid: one vendor, two addresses, one invoice
    ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/38CD17B980D0DDD29E982686519B815BC6274EF5DDDD3CD4610697DF0FE8161A), contract 0.5.1).
  - Two-person approval needs two people: naming that main account and its sub-account as two co-approvers is
    refused, `holds ME ID …, the same person` ([tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/5B3F1F9F297A3F8D3BD12762A67CCE2EBDD369C9597F27537726DE65C031267F), contract 0.5.1).
  - Held above the threshold; the agent can't release it; the owner approves. A `multi_send` batch. Revoke → refused,
    resume → paid.
  - An agent key that **never held MEC** pays on the owner's fee grant (the tx's fee granter is the owner:
    [tx](https://beta-hub-1317.explorer-testnet.me/cosmos/tx/v1beta1/txs/BFCE54579D3AE2F525BB4D5FAB66F3A43019C31F404492693296538E2C87A8B6)) and can't send anything else.

- **Claude Code as the agent, on testnet** ([`runs/testnet-claude-2026-11-25T16-59-00Z.jsonl`](runs/testnet-claude-2026-11-25T16-59-00Z.jsonl)): paid INV-1001, sent INV-1002 for
  approval, declined the duplicate, the fake CEO and the injected invoice. It held MCC-77 for a human: the merchant's
  ME ID checked out, but nothing showed the credits were ordered. (In the 0.5.0 run it paid MCC-77; both are fine.)
- **The obey-everything client, on testnet** ([`runs/testnet-obey-2026-11-25T17-02-39-964Z.txt`](runs/testnet-obey-2026-11-25T17-02-39-964Z.txt)), on the leash Claude had
  just worked: the fake CEO wire, the injected invoice, the limit raise, the withdraw, the one-shot drain and the
  re-spelled duplicates were all refused on chain, and so was the split drain (5 × 0.014 MEC): only 0.03 MEC of the
  0.1 MEC 24-hour limit was left. 8 attacks, 0 through. Splits under the approval threshold are bounded by that
  limit, not stopped ([T6](THREAT_MODEL.md)): in our earlier run on contract 0.4, with a 0.2 MEC limit, the same
  batch to the owner's own vendor was paid ([`runs/superseded-v0.4/`](runs/superseded-v0.4/)).
- **Agent-to-agent commerce, on testnet** ([`runs/testnet-commerce-2026-11-25T17-02-46Z.txt`](runs/testnet-commerce-2026-11-25T17-02-46Z.txt)): HTTP 402, paid through the leash, replay,
  front-running, an unissued invoice and a seller without an ME ID all refused.
- **The red-team sandbox's leash is live on testnet** (agent `me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan`, holds
  0 MEC, fees on the owner's grant).

The kill switch needs the global DAO's key to suspend an ME ID, so it is shown on a **local ME Hub node built from
`openmetaearth/me-hub`** (see below), with repeats of the obey-everything, Claude and commerce runs. CI repeats the
obey-everything, kill-switch and commerce runs on a fresh node on every push; the Claude run is recorded once and
committed.

## By judging criterion

**Technical implementation (30)**
- Contract: [`contracts/leash-id/src`](contracts/leash-id/src). 35 unit tests, including a random-operation test of
  the accounting invariants (40 seeds × 250 operations: contract balance = sum of owner balances, reserved = sum
  of held payments, with partial two-person approvals mixed in).
- [CI](.github/workflows/ci.yml), three jobs on every push:
  - `contract`: tests, clippy, JS tests, npm audit, a reproducible wasm, a float-instruction guard, and a
    byte-for-byte rebuild of the browser bundle.
  - `devnet`: builds me-hub, runs the attack replay, the kill switch, the sub-account and two-signature flows
    and agent-to-agent commerce on a real node. [`scripts/check-runs.mjs`](scripts/check-runs.mjs) then
    asserts each outcome, and the owner console is driven in Chrome.
  - `testnet`: `verify`, and `check:chain`: every testnet tx the committed records link to is fetched from the
    chain and must show the outcome (paid / refused, with the contract's reason) and the contract the record says.
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
- Cost: about 260k gas per payment ([docs/GAS.md](docs/GAS.md)), and a refusal costs nothing by default: it is caught in simulation and never sent. (The recorded demos send refusals on purpose, `LEASH_ONCHAIN_REFUSALS=1`, so you can look them up; those pay a fee.)

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
npm run test:js                   # 24 JS tests, no chain needed
node scripts/check-runs.mjs       # the committed run records say what we claim
npm run check:chain               # ...and every testnet tx they link to says the same on chain (read-only)
cat runs/testnet-claude-*.jsonl | jq -r 'select(.type=="result").result'  # Claude's own report (testnet)
```

## Known limits, stated plainly

- Suspending an ME ID needs the global DAO or the KYC issuer, so the kill switch is shown on a node we run, not on
  testnet (`scripts/testnet-killswitch.sh` waits for it on testnet and records the refusals when it happens). Every
  other flow is recorded on testnet too, and CI re-runs the obey-everything, kill-switch and commerce runs on a
  fresh node.
- The owner console's Keplr path is written to Keplr's API but was tested with a mnemonic signer (`npm run
  e2e:owner`). Bonds are covered by unit tests against the real x/wstaking response shape. A local node has no
  fixed-deposit configuration to try them against.
- The threat model lists what Leash ID does not protect against: [THREAT_MODEL.md](THREAT_MODEL.md).
