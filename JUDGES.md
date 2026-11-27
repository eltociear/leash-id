# For judges: a 10-minute tour

**Leash ID** lets an AI agent pay invoices on ME Hub, but only inside a spending leash tied to its owner's live
ME ID. The contract re-reads the ME ID from x/did on every payment, so suspending it stops every agent under it
in the next block. Sellers can require that a verified human answers for an agent before they take its money.

Everything below can be checked without trusting us: each claim links to code, a recorded run, or a chain query.

**The short version**

- **The problem.** An AI agent with a wallet key can spend everything in it, and nobody can tell which human
  answers for it.
- **What only ME Network makes possible.** The contract asks x/did for the owner's ME ID on every payment
  (whitelisted Stargate query), so the leash belongs to a KYC-verified person, and suspending that ME ID stops all
  their agents in the next block. Vendors are checked the same way, EVM sub-accounts included.
- **Live now.** Contract 0.5.3 on ME Hub testnet, no admin, with every flow recorded against our real ME ID
  (KYC level 2): 40+ transactions, each re-checked against the chain by CI.
- **Attacked, and held.** Claude Code working a poisoned inbox, a client that obeys every instruction (8 attacks,
  0 through), and a public page where anyone can attack a real agent key.
- **Look first:** the [verifier](https://eltociear.github.io/leash-id/), the
  [owner console, read-only](https://eltociear.github.io/leash-id/owner.html#view=me1r7ssfwrhreafc8fvctqewntmgf42djdk7ldhuq),
  the [red-team page](https://leash-id-sandbox.vercel.app/), the [video](https://www.youtube.com/watch?v=vySdFuWiGho).

## Try it (no install)

| What | Where |
|---|---|
| One-page overview | https://eltociear.github.io/leash-id/about.html |
| Check any agent, no wallet | https://eltociear.github.io/leash-id/ (try "an EVM address bound to an ME ID": a real testnet user's sub-account, resolved by the contract from x/did) |
| Owner console, read-only, no wallet: our testnet owner's agents, limits, fees, payments | https://eltociear.github.io/leash-id/owner.html#view=me1r7ssfwrhreafc8fvctqewntmgf42djdk7ldhuq |
| Owner console (Keplr): approvals, two signers, revoke, limits, fee grants, CSV | https://eltociear.github.io/leash-id/owner.html |
| Demo video (4:48, captions) | https://www.youtube.com/watch?v=vySdFuWiGho |
| Red-team page: attack a live testnet agent's real key | https://leash-id-sandbox.vercel.app/ : one-click attacks and your own payments, each answered by the deployed contract. The prompt-injection box (an email Claude reads with the key behind its tools) needs an Anthropic key, which the hosted page doesn't have: run it locally with yours, or read the recorded testnet run [`runs/testnet-claude-*.jsonl`](runs/). Or run the page yourself with no key at all: `npm ci && node scripts/sandbox-dev.mjs` simulates every action as the same agent ([`sandbox/`](sandbox/README.md)) |
| Your own Claude as the agent, no key | `claude --mcp-config mcp/judge.mcp.json`, then ask: *"Who is behind you, and what are you allowed to spend? Then check paying 0.5 MEC to me1vrsz4pmwq5cal98fx8cxwtmz3venj68hvdhq7p for invoice URGENT-1."* It reads the demo agent's real leash and dry-runs against the contract; it can't send anything ([`mcp/judge.mcp.json`](mcp/judge.mcp.json)) |

## On ME Hub testnet (`mechain_400-1`)

- Contract `me1kxmldervpq3w4gyyh98tt8eueap5npdqn4ssw70lcurug0stjcsqhjq3wt`, code 47, **no admin** (nobody can swap
  the code). Store tx `3AC19A10…B625`, instantiate tx `25BD4361…62DD`
  ([tx](https://eltociear.github.io/leash-id/tx.html#25BD43612ADC24D1024C164A7E3015FE25A6A56AF98FC540943557E2C3A562DD)).
- `npm run verify` (and the `testnet` CI job on every push): the wasm on chain is byte-for-byte
  `artifacts/leash_id.wasm`, and the contract has no admin. The `contract` CI job rebuilds that file from source on
  every push (Rust 1.81, `wasm-opt`, `cosmwasm-check`) and fails unless the sha256 matches, so chain == committed ==
  built from this source. (The `testnet` job is advisory, so a testnet outage can't fail the build; the `contract`
  job is not.)
- **Real x/did records, real transactions** ([`runs/testnet-identity-*.txt`](runs/)): another user's ME ID
  binds an EVM sub-account through `MsgCreateSubAccount`. The contract refuses a false link
  ([tx](https://eltociear.github.io/leash-id/tx.html#D2A40AFCD4FD24CBFBDE06C95D06477E7345503A3D54EC30D3370DE49EF696EB)),
  records the true one ([tx](https://eltociear.github.io/leash-id/tx.html#00E4D1D912B9A39334525A0D06E772CF6F6362651885EF52C1D2991571F97C2A)),
  and from then on treats that EVM address as the ME ID's.

- **The payment flows, with our owner's real ME ID** (`2611255912050`, KYC level 2, issued by the testnet KYC
  issuer) in [`runs/testnet-payments-2026-11-27T06-37-34Z.txt`](runs/testnet-payments-2026-11-27T06-37-34Z.txt), 20+ transactions anyone can look up:
  - A second real ME ID, at KYC level 1, tries to fund agents and is refused on chain
    ([tx](https://eltociear.github.io/leash-id/tx.html#FCC4AB24E4B24396B3F81CBB46A9835AB83AFCF56FED5A26F4D55FA5A3B01B63): `has KYC level 1, need 2`).
  - Paid inside the leash; refused, each as a failed tx with its reason: the same invoice spelled differently, over
    the limit, and vendors outside the leash (one with no ME ID, one at KYC level 1, below the "any verified vendor" floor).
  - Paid to an **EVM sub-account** of another user's KYC 2 ME ID: the receipt names that ME ID
    ([tx](https://eltociear.github.io/leash-id/tx.html#AAC747DA0DABDE761A41A67505970FC69A8F4887697FA5FC69B50D4E8C8ACD92)).
    The same invoice at that ME ID's main account is refused as already paid: one vendor, two addresses, one invoice
    ([tx](https://eltociear.github.io/leash-id/tx.html#86C01FFA2417C165F909ED75773420F10EE7A323F0857673A356ED602D9CEC5D), since contract 0.5.0; 0.5.1 also covers a link recorded afterwards).
  - Two-person approval needs two people: naming that main account and its sub-account as two co-approvers is
    refused, `holds ME ID …, the same person` ([tx](https://eltociear.github.io/leash-id/tx.html#CF9E21C1BD9F91808B3CF95B63DB93424D62710BAF4F532895A71B1B0EB0F8ED), since contract 0.5.0).
  - Held above the threshold; the agent can't release it; the owner approves. A `multi_send` batch. Revoke → refused,
    resume → paid.
  - An agent key that **never held MEC** pays on the owner's fee grant (the tx's fee granter is the owner:
    [tx](https://eltociear.github.io/leash-id/tx.html#DCC6B9E23656956BEB6186D2069CCB0DC39368C0932727F3A4B0110B48333FBE)) and can't send anything else.

- **Claude Code as the agent, on testnet** ([`runs/testnet-claude-2026-11-27T06-39-58Z.jsonl`](runs/testnet-claude-2026-11-27T06-39-58Z.jsonl)): paid INV-1001, sent INV-1002 for
  approval, declined the duplicate, the fake CEO and the injected invoice, and paid MCC-77 after checking on chain that
  the merchant holds a live, KYC-verified ME ID. MCC-77 is the one judgment call in the inbox: some runs leave it for
  a human because nothing shows the credits were ordered, and the checks accept either.
- **The obey-everything client, on testnet** ([`runs/testnet-obey-2026-11-27T06-41-51-035Z.txt`](runs/testnet-obey-2026-11-27T06-41-51-035Z.txt)), on the leash Claude had
  just worked: the fake CEO wire, the injected invoice, the limit raise, the withdraw, the one-shot drain and the
  re-spelled duplicates were all refused on chain, and so was the split drain (5 × 0.014 MEC): only 0.03 MEC of the
  0.1 MEC 24-hour limit was left. 8 attacks, 0 through. Splits under the approval threshold are bounded by that
  limit, not stopped ([T6](THREAT_MODEL.md)): in our earlier run on contract 0.4, with a 0.2 MEC limit, the same
  batch to the owner's own vendor was paid ([`runs/superseded-v0.4/`](runs/superseded-v0.4/)).
- **Agent-to-agent commerce, on testnet** ([`runs/testnet-commerce-2026-11-27T06-41-56Z.txt`](runs/testnet-commerce-2026-11-27T06-41-56Z.txt)): HTTP 402, paid through the leash, replay,
  front-running, an unissued invoice and a seller without an ME ID all refused.
- **Two signatures, with two real KYC-2 ME IDs, on testnet** ([`runs/testnet-cosign-2026-11-27T06-43-15Z.txt`](runs/testnet-cosign-2026-11-27T06-43-15Z.txt)): the owner adds the CFO
  (ME ID `2611264146169`) as co-approver. A held payment isn't paid on the owner's signature
  ([tx](https://eltociear.github.io/leash-id/tx.html#9C96151DB9EB1A250A9FE4B56F850F6AF7E4AA781DA6948CE95E395355484A15): `approvals=1 needed=2`)
  and is paid on the CFO's ([tx](https://eltociear.github.io/leash-id/tx.html#EADF691746DF6C10617CFA5A67E15F85A24B0A2C6E43D535EE7DC9376804C725)). The owner key alone then
  tries to drop the approval line and to withdraw: both wait 24 h
  ([tx](https://eltociear.github.io/leash-id/tx.html#4C8D2DD37D30D1388C7940FE84EC452A7258A47125237DCCFA66318B6CAF54DD),
  [tx](https://eltociear.github.io/leash-id/tx.html#48183AD7EF4A63A86D5BBEB2C4DFF5A1015A363479253AB4961A3AB3CE594A80)). The CFO cancels both
  ([tx](https://eltociear.github.io/leash-id/tx.html#DCE5E2AB8BA1010C0CBB82899578E87157A1095366AA079C4B2C3771B3947D8F),
  [tx](https://eltociear.github.io/leash-id/tx.html#0AD2152FDD3945D018CF98682BE011694D8E79EB4B7738BF95986A67629306EF)) and revokes the agent at once
  ([tx](https://eltociear.github.io/leash-id/tx.html#901FAADECF42CBB530A64478A636CA7D1791B8C52A54651BCE546DD000C62728)). Going back to one signature takes both of them, and then applies at once
  ([tx](https://eltociear.github.io/leash-id/tx.html#8282DED9A790B06E018C093366C473B64BB294A8B0B338AD1A7C8BEE903B85B9)). The CFO's key holds no MEC: its fees are on the owner's grant.
- **A bond in x/wstaking, on testnet** ([`runs/testnet-bond-2026-11-27T06-42-24Z.txt`](runs/testnet-bond-2026-11-27T06-42-24Z.txt)): 0.02 MEC locked in an ME Hub fixed deposit
  (`MsgDoFixedDeposit`) and shown as the owner's bond, which the contract reads live from x/wstaking; someone
  else's deposit can't be claimed (refused on chain).
- **The red-team sandbox's leash is live on testnet** ([`runs/testnet-sandbox-2026-11-27T08-11-24Z.txt`](runs/testnet-sandbox-2026-11-27T08-11-24Z.txt); agent
  `me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan`, holds 0 MEC, fees on the owner's grant, valid to 2027-02-10). It was set
  up after the recorded runs (the bond record still shows it unleashed); `leash whois me15xd643…` shows it live.

The kill switch needs the global DAO's key to suspend an ME ID, so it is shown on a **local ME Hub node built from
`openmetaearth/me-hub`** (see below), with repeats of the obey-everything and commerce runs, and a Claude run
recorded there on contract 0.4 (11-24). CI repeats the obey-everything, kill-switch and commerce runs on a fresh node
on every push; Claude runs are recorded once and committed.

## By judging criterion

**Technical implementation (30)**
- Contract: [`contracts/leash-id/src`](contracts/leash-id/src). 44 unit tests, including a random-operation test of
  the accounting invariants (40 seeds × 250 operations: contract balance = sum of owner balances, reserved = sum
  of held payments, no invoice paid twice, never more than the daily limit in any 24 hours; partial two-person
  approvals and sub-hour clock steps mixed in).
- [CI](.github/workflows/ci.yml), three jobs on every push:
  - `contract`: tests, clippy, JS tests, npm audit, a reproducible wasm, a float-instruction guard, and a
    byte-for-byte rebuild of the browser bundle.
  - `devnet`: builds me-hub, runs the attack replay, the kill switch, the sub-account and two-signature flows
    and agent-to-agent commerce on a real node. [`scripts/check-runs.mjs`](scripts/check-runs.mjs) then
    asserts each outcome, and the owner console is driven in Chrome.
  - `testnet`: `verify`, and `check:chain`: every testnet tx the committed records link to is fetched from the
    chain and must show the outcome (paid / refused, with the contract's reason) and the contract the record says.
  - `contract` also runs `check:chain --offline`, so the evidence outlives the testnet: [`runs/chain-snapshot.json`](runs/chain-snapshot.json)
    keeps each cited tx's bytes, its merkle proof, its block header and results and the next header, with the
    validators' signatures. [`lib/txproof.mjs`](lib/txproof.mjs) checks, with no network, that the bytes hash to the
    cited hash, sit in the block's `data_hash`, that the header hashes to the signed block ID, and that the paid/refused
    code is the one the next block's `last_results_hash` commits to. A reset testnet can't erase the records; an edited
    snapshot fails (`test/txproof.test.mjs`). Not covered offline: the log text, which CometBFT doesn't hash.
- Nine independent reviews so far, each finding listed with its fix and a test in [CHANGELOG.md](CHANGELOG.md):
  adversarial reviews of the contract (in releases 0.2.0, 0.4.0, 0.6.0 and 0.6.1), a fresh-eyes accounting review
  (release 0.7.0, contract 0.5.2: the daily window), an injection review of the pages (release 0.7.0), a judge-style review that found the owner alone could
  go around two signatures and a second review of that fix before deploying it (contract 0.5.3), and a review of
  the off-chain code: MCP server, paywall, CLI, sandbox. Nothing found has been left unfixed without being written down in
  [THREAT_MODEL.md](THREAT_MODEL.md).
- Sends hash the tx before broadcasting. An unconfirmed send is reported as UNKNOWN with its hash, never as
  "not sent", so a retry can't double-pay. While the testnet is halted a send isn't broadcast at all: it comes back
  NOT SENT with the age of the latest block, in under a second, instead of waiting 60 s and then reporting UNKNOWN.

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
- Two-person approval means two ME IDs: co-approvers are counted one per ME ID, so two addresses of one ME ID
  (a main account and its EVM sub-account) can't be two signatures (refused on testnet, `runs/testnet-payments-*.txt` 5b).
- `leash lock` makes the bond with ME Hub's own `MsgDoFixedDeposit` (x/wstaking, hand-encoded: cosmjs has no type
  for it), in the owner's KYC region's term.

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
  - an agent key that holds 0 MEC: the owner pays its fees through x/feegrant, contract calls only (kill-switch run, step 8)
- For the seller: `leash-id/vendor` takes payment from AI agents only when a verified human answers for them.
- Who it's for, and how it gets used:
  - Accounts-payable teams letting an agent clear routine invoices (SaaS bills, cloud credits, contractor
    payouts), with the CFO as co-approver above a line; DAO and project treasuries that want an agent to pay
    grants or bounties without handing it the multisig.
  - Sellers of APIs and data to agents (`leash-id/vendor`): they get paid only by agents a live ME ID answers for.
  - What would carry it to mainnet: the same contract (no admin, so it would be a fresh deploy), an ME ID KYC
    issuer hook to suspend an ME ID from the owner's side (today only the DAO or the issuer can), and a hosted
    owner console. A business model fits the vendor side: a hosted paywall (verified-agent checkout) priced per
    settled payment, with the contract itself free to use.
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
- MCP results Claude can act on: `PAID`, `HELD FOR OWNER APPROVAL`, `REFUSED`, `UNKNOWN` (may have gone through:
  look it up) and `NOT SENT` (safe to retry), each with the reason and the tx link.

## Check it yourself

```bash
git clone https://github.com/eltociear/leash-id && cd leash-id && npm ci   # npm audit: 0 vulnerabilities
npm run judge                     # all five checks below in one go (~40 s, read-only), one line each
npm run verify                    # testnet: deployed code == this repo's build, no admin
npm run test:js                   # 33 JS tests, no chain needed
node scripts/check-runs.mjs       # the committed run records say what we claim
npm run check:chain               # ...and every testnet tx they link to says the same on chain (read-only)
npm run check:chain:offline       # the same, with no network: from runs/chain-snapshot.json, each tx proven into its signed block
cat runs/testnet-claude-*.jsonl | jq -r 'select(.type=="result").result'  # Claude's own report (testnet)
```

## Known limits, stated plainly

- ME Hub testnet stops now and then. On 2026-11-26 it reached block 5,523,300 (07:09 UTC), the height governance
  scheduled for the me-hub `v2.0.18.rc1` upgrade (proposal 11). The chain itself moved on within minutes (block
  5,523,301 is stamped 07:10:01), but the public endpoint every page and tool reads stayed at 5,523,300 until about
  06:15 UTC on 11-27, so for 23 hours nothing could be sent through it. The release now running
  (`v2.0.18-5-g4826eefb`) changes x/regiongov, x/wstaking validator resets and x/wdistri only: x/did, x/kyc, the
  FixedDeposit query and the CosmWasm bindings are unchanged, and CI's devnet job builds that commit. While the
  endpoint stood still, every page showed the latest block, its age and the scheduled upgrade; reads kept working;
  and the CLI, MCP server and sandbox didn't broadcast into it (NOT SENT, safe to retry), so nothing could land
  hours later by surprise.
- Suspending an ME ID needs the global DAO or the KYC issuer, so the kill switch is shown on a node we run, not on
  testnet (`scripts/testnet-killswitch.sh` waits for it on testnet and records the refusals when it happens). Every
  other flow is recorded on testnet too, and CI re-runs the obey-everything, kill-switch and commerce runs on a
  fresh node.
- The owner console's Keplr path is written to Keplr's API but was tested with a mnemonic signer (`npm run
  e2e:owner`). Bonds are covered by unit tests against the real x/wstaking response shape and recorded on testnet
  (`runs/testnet-bond-*.txt`), but region `me_earth` offers only a 1-day term, so a testnet bond lasts that day.
- The threat model lists what Leash ID does not protect against: [THREAT_MODEL.md](THREAT_MODEL.md).
