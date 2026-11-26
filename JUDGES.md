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
- **Live now.** Contract 0.5.2 on ME Hub testnet, no admin, with every flow recorded against our real ME ID
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

- Contract `me1dsl645wcgyf5cckkrrcmxsa255r5xve80nm95v8azmc3hnk5uwrqvwsrnp`, code 46, **no admin** (nobody can swap
  the code). Store tx `D1560CC9…954E`, instantiate tx `C494CA4C…64A2`
  ([tx](https://eltociear.github.io/leash-id/tx.html#C494CA4C146E346A3B4739EB4391198E842A9DC2A7CC0B4CA756667E983564A2)).
- `npm run verify` (and the `testnet` CI job on every push): the wasm on chain is byte-for-byte
  `artifacts/leash_id.wasm`, and the contract has no admin. The `contract` CI job rebuilds that file from source on
  every push (Rust 1.81, `wasm-opt`, `cosmwasm-check`) and fails unless the sha256 matches, so chain == committed ==
  built from this source (the CI job is advisory, so a testnet outage can't fail the build).
- **Real x/did records, real transactions** ([`runs/testnet-identity-*.txt`](runs/)): another user's ME ID
  binds an EVM sub-account through `MsgCreateSubAccount`. The contract refuses a false link
  ([tx](https://eltociear.github.io/leash-id/tx.html#E3469C6EAEF2043D93CB135D3E5839F31AD17F9D3D5B1A4D3B4F3CE1F5DBE657)),
  records the true one ([tx](https://eltociear.github.io/leash-id/tx.html#EC2AAB226921167BF9CF235907DF68563DDC6324A2180D98946A4A83B5318C39)),
  and from then on treats that EVM address as the ME ID's.

- **The payment flows, with our owner's real ME ID** (`2611255912050`, KYC level 2, issued by the testnet KYC
  issuer) in [`runs/testnet-payments-2026-11-26T06-16-31Z.txt`](runs/testnet-payments-2026-11-26T06-16-31Z.txt), 20+ transactions anyone can look up:
  - A second real ME ID, at KYC level 1, tries to fund agents and is refused on chain
    ([tx](https://eltociear.github.io/leash-id/tx.html#B545D8FE357E0FD5A147AF5A12C043F321A37FE9E10A430F74ACD9BB64FBD509): `has KYC level 1, need 2`).
  - Paid inside the leash; refused: the same invoice spelled differently, over the limit, a vendor with no ME ID, a
    vendor at KYC level 1. Each refusal is a failed tx with its reason.
  - Paid to an **EVM sub-account** of another user's KYC 2 ME ID: the receipt names that ME ID
    ([tx](https://eltociear.github.io/leash-id/tx.html#B35B41CBDF92610D753B69E3EDBD514827C90DDB019DD3955E7D60FAC5560B92)).
    The same invoice at that ME ID's main account is refused as already paid: one vendor, two addresses, one invoice
    ([tx](https://eltociear.github.io/leash-id/tx.html#F0FBC7C34A803E23FF99C55553A0960B1CF46F3C2813475DC761AEA0719DC4C2), since contract 0.5.1).
  - Two-person approval needs two people: naming that main account and its sub-account as two co-approvers is
    refused, `holds ME ID …, the same person` ([tx](https://eltociear.github.io/leash-id/tx.html#97D71047C9707CA2DDCBB2ED0B003EBFC0889C37630F3B0B0409C7267F3546FF), since contract 0.5.1).
  - Held above the threshold; the agent can't release it; the owner approves. A `multi_send` batch. Revoke → refused,
    resume → paid.
  - An agent key that **never held MEC** pays on the owner's fee grant (the tx's fee granter is the owner:
    [tx](https://eltociear.github.io/leash-id/tx.html#E6C8E669F2B6FC817C2B5630A120C8CB21D4C08E81DC59B3187072B0676A7A6D)) and can't send anything else.

- **Claude Code as the agent, on testnet** ([`runs/testnet-claude-2026-11-26T06-19-06Z.jsonl`](runs/testnet-claude-2026-11-26T06-19-06Z.jsonl)): paid INV-1001, sent INV-1002 for
  approval, declined the duplicate, the fake CEO and the injected invoice, and paid MCC-77 after checking on chain that
  the merchant holds a live, KYC-verified ME ID. MCC-77 is the one judgment call in the inbox: some runs leave it for
  a human because nothing shows the credits were ordered, and the checks accept either.
- **The obey-everything client, on testnet** ([`runs/testnet-obey-2026-11-26T06-20-59-836Z.txt`](runs/testnet-obey-2026-11-26T06-20-59-836Z.txt)), on the leash Claude had
  just worked: the fake CEO wire, the injected invoice, the limit raise, the withdraw, the one-shot drain and the
  re-spelled duplicates were all refused on chain, and so was the split drain (5 × 0.014 MEC): only 0.03 MEC of the
  0.1 MEC 24-hour limit was left. 8 attacks, 0 through. Splits under the approval threshold are bounded by that
  limit, not stopped ([T6](THREAT_MODEL.md)): in our earlier run on contract 0.4, with a 0.2 MEC limit, the same
  batch to the owner's own vendor was paid ([`runs/superseded-v0.4/`](runs/superseded-v0.4/)).
- **Agent-to-agent commerce, on testnet** ([`runs/testnet-commerce-2026-11-26T06-21-04Z.txt`](runs/testnet-commerce-2026-11-26T06-21-04Z.txt)): HTTP 402, paid through the leash, replay,
  front-running, an unissued invoice and a seller without an ME ID all refused.
- **The red-team sandbox's leash is live on testnet** (agent `me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan`, holds
  0 MEC, fees on the owner's grant).

The kill switch needs the global DAO's key to suspend an ME ID, so it is shown on a **local ME Hub node built from
`openmetaearth/me-hub`** (see below), with repeats of the obey-everything, Claude and commerce runs. CI repeats the
obey-everything, kill-switch and commerce runs on a fresh node on every push; the Claude run is recorded once and
committed.

## By judging criterion

**Technical implementation (30)**
- Contract: [`contracts/leash-id/src`](contracts/leash-id/src). 39 unit tests, including a random-operation test of
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
- Two-person approval means two ME IDs: co-approvers are counted one per ME ID, so an owner's own EVM sub-account
  can't be their second signature (refused on testnet, `runs/testnet-payments-*.txt` 5b).
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
  - an agent key that holds 0 MEC: the owner pays its fees through x/feegrant, contract calls only (step 9)
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
- MCP results Claude can act on: `PAID`, `HELD FOR OWNER APPROVAL`, `REFUSED`, `UNKNOWN`.

## Check it yourself

```bash
git clone https://github.com/eltociear/leash-id && cd leash-id && npm ci   # its advisories are in puppeteer (browser e2e only); npm audit --omit=dev: 0
npm run judge                     # all four checks below in one go (~40 s, read-only), one line each
npm run verify                    # testnet: deployed code == this repo's build, no admin
npm run test:js                   # 26 JS tests, no chain needed
node scripts/check-runs.mjs       # the committed run records say what we claim
npm run check:chain               # ...and every testnet tx they link to says the same on chain (read-only)
cat runs/testnet-claude-*.jsonl | jq -r 'select(.type=="result").result'  # Claude's own report (testnet)
```

## Known limits, stated plainly

- ME Hub testnet halts now and then (it stopped at block 5,523,300, 07:09 UTC on 2026-11-26). Every page shows
  the latest block and its age and says so when the chain isn't moving: reads still work, new transactions wait.
- Suspending an ME ID needs the global DAO or the KYC issuer, so the kill switch is shown on a node we run, not on
  testnet (`scripts/testnet-killswitch.sh` waits for it on testnet and records the refusals when it happens). Every
  other flow is recorded on testnet too, and CI re-runs the obey-everything, kill-switch and commerce runs on a
  fresh node.
- The owner console's Keplr path is written to Keplr's API but was tested with a mnemonic signer (`npm run
  e2e:owner`). Bonds are covered by unit tests against the real x/wstaking response shape, and `leash lock` was
  checked by simulation on testnet (region `me_earth` offers a 1-day term). A bond on testnet only lasts that day;
  the recorded one goes in when the halted chain is back.
- The threat model lists what Leash ID does not protect against: [THREAT_MODEL.md](THREAT_MODEL.md).
