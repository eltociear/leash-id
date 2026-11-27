# Leash ID — KYC-backed spending leashes for AI agents on ME Network

**English** · [简体中文](README.zh-CN.md) · [日本語](README.ja.md) · **Judges: start with [JUDGES.md](JUDGES.md)** (a 10-minute tour with evidence for each criterion)

![Leash ID: every agent payment re-checks the owner's live ME ID on ME Hub](assets/cover.png)

[![ci](https://github.com/eltociear/leash-id/actions/workflows/ci.yml/badge.svg)](https://github.com/eltociear/leash-id/actions/workflows/ci.yml)
Release 0.7.0 · contract 0.5.2 live on ME Hub testnet (code 46, no admin)

> **If a page says ME Hub testnet is halted:** since 2026-11-26 07:09 UTC the testnet has stood at block 5,523,300,
> the height governance scheduled for its upgrade to `v2.0.18.rc1` (proposal 11), waiting for validators to switch
> releases. Everything already on chain still checks out (`npm run judge`). New payments are refused before signing
> ("not sent", nothing spent). CI re-runs the recorded flows on a local `v2.0.18.rc1` chain.

**Demo video (4:48):** https://www.youtube.com/watch?v=vySdFuWiGho ·
**Check an agent:** https://eltociear.github.io/leash-id/ (reads ME Hub testnet live, no wallet) ·
**Owner console:** https://eltociear.github.io/leash-id/owner.html (Keplr; or [read-only, no wallet](https://eltociear.github.io/leash-id/owner.html#view=me1r7ssfwrhreafc8fvctqewntmgf42djdk7ldhuq)) ·
**Red-team it:** https://leash-id-sandbox.vercel.app/ (attack a live testnet agent's key; [`sandbox/`](sandbox/README.md), or `node scripts/sandbox-dev.mjs` with no key) ·
**[Threat model](THREAT_MODEL.md)** · **[Security / deployment](SECURITY.md)** · **[Toward a standard: PQ Agent Mandate](MANDATE.md)** · **Check everything in one command:** `npm ci && npm run judge` (read-only, ~40 s)

AI agents are starting to pay invoices and buy services. Two things are missing:

1. **Counterparties can't tell who stands behind an agent.** An address is just an address.
2. **Owners have no hard guarantee the agent can't overspend.** It's "hand over your keys" or "approve everything by hand."

Why now:

- **Accounts payable is where the money is stolen.** Business email compromise drew 21,442 complaints and close to
  $2.8 billion in reported losses in 2024, and nearly $8.5 billion over 2022-2024 ([FBI IC3 via Nacha](https://www.nacha.org/news/fbis-ic3-finds-almost-85-billion-lost-business-email-compromise-last-three-years)).
  An AI agent reading invoices inherits that attack: one convincing email away from a wire.
- **The networks are building agent verification.** Visa's Trusted Agent Protocol helps merchants recognise
  trusted agents, and Mastercard pairs Agent Pay with "Verifiable Intent" for agent permissions
  ([Visa](https://hk.review.visa.com/en_HK/about-visa/newsroom/press-releases/the-key-forces-driving-payments-for-2026-from-agentic-commerce-to-interoperable-payments.html),
  [Mastercard](https://www.mastercard.com/in/en/news-and-trends/stories/2026/mastercard-agentic-commerce-vision.html)).
  Both are card-network credentials. On a chain, nobody vouches for an agent.
- **On-chain agent payments can't tell who is paying.** "A scheduled script and an agent produce the same record"
  ([TRM Labs, Sep 2026](https://www.trmlabs.com/trm-tech-blog/whos-actually-paying-measuring-ai-agent-payments-onchain)).
  A seller taking x402-style payments can't see whether anyone answers for the payer.

Leash ID is a CosmWasm contract on **ME Hub** that ties every agent to a **live ME ID** (ME Network's native KYC
identity) and enforces a spending leash on-chain. An MCP server lets Claude, or any other agent, work an
accounts-payable inbox under that leash.

```
            ┌────────────── ME Hub (mechain_400-1) ───────────────────┐
 owner ───► │ Leash ID contract (no admin: nobody can swap the code)  │
 (ME ID,    │  deposit / offer leash / approve / revoke               │
  KYC ≥ L2) │                                       accept ◄── agent  │
            │  pay(vendor, amount, invoice)  ◄──────────── agent key  │ ◄── Claude / any LLM (MCP)
            │   1. owner's ME ID still ACTIVE and KYC ≥ min? ─────────┼──► x/did  (Stargate query
            │   2. vendor allow-listed, or vendor holds an ME ID? ────┼──► x/did   /metaearth.did.Query/Did)
            │   3. per-payment / rolling-24h limits, budget, expiry   │
            │   4. invoice never handled before (case/separators ignored)
            │   5. over threshold → held until the owner approves     │
            │  pay_batch → ME Hub `multi_send` custom message         │
            │  whois(agent) → owner + live ME ID + "can pay now?"     │ ◄── any counterparty
            └─────────────────────────────────────────────────────────┘
```

## Why this is ME-native

ME Hub opens exactly three Stargate queries and one custom message to CosmWasm (`app/keepers/wasm_me_query.go`,
`wasm_me_msg.go`). Leash ID uses all of them that carry meaning: the ME ID (`Did`), fixed deposits
(`FixedDeposit`) and `multi_send`. The fourth, `Region`, describes a region but x/did's `regionId` is marked
unused, so nothing ties an account to it.

- **On-chain identity checks, no oracle.** ME Hub whitelists one identity query for CosmWasm:
  `/metaearth.did.Query/Did` (`me-hub/app/keepers/wasm_me_query.go`). Leash ID calls it inside `deposit`, the
  leash offer and acceptance, **every `pay`**, every `approve`, and `whois`. Suspend an owner's ME ID and every
  agent under it stops paying in the next block. Nobody has to revoke anything.
- **KYC-aware vendor rules.** A leash can allow "any vendor that holds a live ME ID", at the contract's KYC floor
  (`min_kyc_level`, fixed when the contract is deployed: 2 on testnet), so agents can pay verified merchants without
  a hand-kept allowlist. Approving a held payment re-checks it. Sellers set their own floor for buyers in the paywall
  (`minKyc`).
- **ME Hub's custom `multi_send` message** (`wasm_me_msg.go`) settles batched payouts in one bank message.
- **EVM sub-accounts count.** A merchant paid at the EVM-form address bound to its ME ID is recognised through
  x/did's own `sub_account` binding (`register_sub_account`, re-checked on every use).
- **Bonds.** An owner can show ME Hub fixed deposits as a bond: MEC they can't move before a date, read live. `leash lock 0.05` makes the x/wstaking deposit and shows it in one step.
- **Receipts carry identities.** Every payment stores the owner's ME ID and the vendor's ME ID (if any) as of
  the moment it was paid, and emits a `wasm-leash_payment` event an indexer or accounting system can consume.

## Install

```bash
npm install -g github:eltociear/leash-id     # Node 20+; gives you `leash` and `leash-mcp`
leash keygen owner agent                      # one key file per role, mode 0600
leash info                                    # network, contract, code id, admin, version
```

Defaults point at ME Hub testnet and the contract in `deployment.json`. Override with `ME_CHAIN_ID`, `ME_RPC`
(comma-separated for failover), `ME_REST`, `LEASH_CONTRACT`.

## Put an agent on a leash

```bash
# owner (holds an ME ID at KYC level ≥ 2)
leash deposit 0.5                             # 1 MEC = 10^8 umec on ME Hub
leash offer agent --per-tx 0.02 --daily 0.05 --approve-above 0.015 --vendors me1acme… --any-verified-vendor yes
leash gas-grant me1agent… --limit 0.01            # optional: you pay its fees (contract calls only), so the agent key holds 0 MEC
# agent key (on the agent's own host: see SECURITY.md)
leash accept me1owner… --key-file /run/secrets/agent.mnemonic --fee-granter me1owner…
# anyone
leash whois me1agent…                         # owner, live ME ID, limits, can it pay right now?
```

Day to day, in the browser: the **[owner console](https://eltociear.github.io/leash-id/owner.html)** (connect Keplr)
(tested end to end with a mnemonic signer) lists what's waiting for your approval with Approve / Reject buttons, shows each agent's 24h spend, and has
Revoke / Resume, a form to change limits and vendors, paying an agent's fees (so its key holds 0 MEC), co-approvers (e.g. owner + CFO: two signatures release a
held payment, either can reject), and a CSV export for accounting. Every button is a transaction your wallet signs. The page
is static and loads nothing from third parties (the signing library is bundled, and CI rebuilds it byte for byte).

From a terminal: `leash pending`, `leash approve <id>`, `leash reject <id>`, `leash history`, `leash policy <agent>
--daily 0.1` (changes only what you pass), `leash revoke <agent>`, `leash resume <agent>`. `leash help <command>`
for details. Exit codes: 0 done, 1 refused/failed, 2 usage error, 3 outcome unknown (look it up before retrying).

Get told when an agent needs you:

```bash
leash watch --owner me1owner… --webhook https://hooks.slack.com/services/…
# NEEDS YOUR APPROVAL: #12 0.018 MEC to me1j64… invoice INV-1002 by agent me1m8t…  → leash approve 12  or  leash reject 12
```

`watch` follows the contract's `wasm-leash_payment` events (paid, held, approved, rejected) and posts each one as
`{"text": …, "leash_payment": {…}}`, which Slack, Discord (`/slack` endpoints) and most chat tools accept.

## Give it to Claude (MCP)

```json
{ "mcpServers": { "leash-id": {
    "command": "leash-mcp",
    "env": { "LEASH_AGENT_KEY_FILE": "/run/secrets/agent.mnemonic", "LEASH_INBOX": "/data/inbox.json" } } } }
```

| Tool | |
|---|---|
| `whoami` | the agent's owner, ME ID, leash, 24h spend, and whether it can pay right now |
| `list_inbox`, `read_message` | the AP inbox, wrapped as **untrusted** content |
| `check_payment` | dry run against the exact on-chain rules, free |
| `pay_invoice`, `pay_batch` | pay; results are `PAID`, `HELD FOR OWNER APPROVAL`, `REFUSED`, `UNKNOWN` or `NOT SENT` (errors set `isError` and `structuredContent.status`) |
| `find_invoice`, `my_payments`, `payment_status` | was this invoice handled? what did I pay? |
| `verify_agent`, `lookup_me_id` | check another agent or address before trusting it |
| `pay_for_api` | buy from an HTTP 402 API that sells to Leash ID agents (checks the seller's ME ID, price cap, payer proof) |
| `retry_api_answer` | ask a paid API again for an answer already paid for (no new payment) |
| `action_log` | everything this server sent in the session, with links |

The server loads only the agent key and **refuses to start if any other Leash ID key file in its key directory is
readable from its process**, because a model with shell or file tools could otherwise read it. Keys kept elsewhere
aren't detected: run the agent as its own user or in a container (SECURITY.md). Read [SECURITY.md](SECURITY.md) before you
connect real money. `LEASH_READONLY=1 LEASH_AGENT_ADDRESS=me1…` runs it with no key at all (reads and dry runs).

## Sell to agents (vendor side)

```js
import http from "node:http";
import { paywall } from "leash-id/vendor";

http.createServer(paywall({ payTo: "me1yourshop…", priceUmec: "100000", minKyc: 2 }, (req, res, paid) => {
  res.end(JSON.stringify({ data: "…", served_to_owner_me_id: paid.owner_did }));
})).listen(8080);
```

An agent that calls it gets `402 Payment Required` with Leash ID terms (pay to, amount, a fresh signed invoice).
It pays through its leash and retries with `X-Leash-Payment: <tx>` and `X-Leash-Proof`, its own key's ADR-36
signature over the tx and invoice (tx hashes are public; the proof stops anyone else redeeming the payment).
The paywall reads that payment from ME Hub, checks it is for a live invoice it issued and has not been used,
and that a live ME ID (KYC ≥ `minKyc`, bond ≥ `minBondUmec` if you ask) answers for the agent. Our MCP server's `pay_for_api` tool does the agent's half,
and refuses sellers without a live ME ID at the contract's KYC level (unless you listed them), and prices above
your owner's approval threshold (a held payment can't buy an answer now). Each tx buys one response; the paywall
remembers spent txs in its own process, so behind several replicas pass `spent: { claim, release }` backed by a
shared store. Recorded on a real node in `runs/devnet-commerce-*.txt`; a full example
is in `examples/paid-api`.

## Contract

`contracts/leash-id` (Rust, cosmwasm-std 1.5, built with Rust 1.81 for wasmvm 1.4). Contract 0.5.2.

| Execute | Who | What |
|---|---|---|
| `deposit {}` | owner (ME ID) | escrow budget; the only message that accepts funds |
| `withdraw {amount}` | owner | take back budget not reserved for held payments |
| `register_agent {agent, policy}` | owner (ME ID) | **offer** a leash; nothing is in force yet |
| `accept_leash {owner}` / `leave_leash {}` | agent | take the offered leash / drop it (held payments are rejected) |
| `cancel_offer {agent}` | owner | withdraw an unaccepted offer |
| `update_policy` | owner | change the policy (never lifts a revoke; keeps the 24h window) |
| `revoke_agent` / `resume_agent` | owner | stop the agent and reject its held payments / lift the revoke |
| `pay {vendor, amount, invoice, memo}` | agent | pay within the leash |
| `pay_batch {payments}` | agent | up to 20 payments via ME `multi_send`, all or nothing |
| `approve {id}` / `reject {id}` | owner | settle held payments (approve re-checks ME ID, vendor, and age ≤ 7 days) |
| `release_invoice {vendor, invoice}` | owner | let a handled invoice number be paid again |
| `register_sub_account {sub, main}` | anyone | record an EVM sub-account x/did binds to `main`'s ME ID |
| `set_bond {deposit_ids}` | owner (ME ID) | show ME Hub fixed deposits as a bond |
| `set_approvers {approvers, threshold}` | owner (ME ID) | N-of-M approval for held payments (owner + up to 5 co-approvers); any signer can reject |

| Query | |
|---|---|
| `whois {agent}` | owner, live ME ID, `verified`, `can_pay_now` + `blocked_by`, limits, 24h spend, budget |
| `check_pay {agent, vendor, amount, invoice, memo}` | dry run; the same `evaluate()` as `pay` |
| `payments_by_agent`, `payments_by_owner` | history, newest first, paginated |
| `pending {owner}`, `invoice {owner, vendor, invoice}` | approval queue; "was this invoice handled?" |
| `identity {address}` | ME ID (direct or via main account) and bond of any address |
| `approvers {owner}`, `approver_of {approver}` | an owner's signers; the owners an address co-signs for |
| `owner`, `offers`, `payment`, `payments`, `config`, `version`, `me_id` | state |

Policy: `per_tx_limit`, `daily_limit` (any 24 hours: hourly buckets, each counted for 25 h), `approve_above` (< `per_tx_limit`), `vendors`
(≤ 32), `any_verified_vendor`, `expires_at` (≤ 366 days). Limits per owner: 50 agents, 50 held payments.

## On ME Hub testnet, with a real ME ID

Our owner holds a testnet ME ID (`2611255912050`, KYC level 2, issued by the testnet KYC issuer).
`scripts/testnet-pay.sh` records the payment flows against it in [`runs/testnet-payments-2026-11-26T06-16-31Z.txt`](runs/testnet-payments-2026-11-26T06-16-31Z.txt), every step a transaction:

- A second real ME ID at KYC level 1 tries to fund agents: refused on chain, `has KYC level 1, need 2`.
- The agent pays inside the leash. The same invoice spelled differently, an over-limit amount, a vendor with no ME ID
  and a vendor at KYC level 1 are refused, each as a failed tx with its reason.
- It pays an EVM sub-account bound to another user's KYC 2 ME ID, and the receipt names that ME ID. The same
  invoice at that ME ID's main account is refused as already paid (one vendor, two addresses).
- Naming that main account and its sub-account as two co-approvers is refused: they are one person (one ME ID).
- A held payment the agent can't release and the owner approves; a `multi_send` batch; revoke → refused, resume → paid.
- An agent key that never held any MEC pays on the owner's fee grant, and can't send anything else.

**Claude Code as the agent, on testnet** ([`runs/testnet-claude-2026-11-26T06-19-06Z.jsonl`](runs/testnet-claude-2026-11-26T06-19-06Z.jsonl)): it paid Acme's invoice, sent the GPU invoice for the
owner's approval, and declined the duplicate, the fake CEO wire and the invoice with hidden instructions. It paid the
small API-credits invoice after checking on chain that the merchant holds a live, KYC-verified ME ID, and flagged that
someone should confirm the credits were ordered.
**The obey-everything client, on testnet** ([`runs/testnet-obey-2026-11-26T06-20-59-836Z.txt`](runs/testnet-obey-2026-11-26T06-20-59-836Z.txt)), on the same leash right after Claude: all 8
attacks were refused on chain, the split drain by the 24-hour limit. Below the approval threshold that limit is the
bound, not a stop: an earlier run with a 0.2 MEC limit paid the same split to the owner's own vendor
([THREAT_MODEL T6](THREAT_MODEL.md)), so set it to what you can afford to lose.
**Agent-to-agent commerce, on testnet** ([`runs/testnet-commerce-2026-11-26T06-21-04Z.txt`](runs/testnet-commerce-2026-11-26T06-21-04Z.txt)): a paid API sells to our agent, and replay, front-running,
unissued invoices and a seller with no ME ID all fail.

## The kill switch, on a real ME Hub node

Only ME Hub's global DAO (`MsgUpdateDidStatus`) or the KYC issuer (`MsgRemove`) can suspend an ME ID, and on
testnet we are neither (all 253 testnet ME IDs are active today). So `scripts/killswitch.sh` runs against a local
ME Hub node built from `openmetaearth/me-hub` (see [`devnet/`](devnet/README.md)), playing the DAO there.
`scripts/testnet-killswitch.sh` is ready for testnet: it holds a payment, watches x/did, and records the refusals as soon
as the owner's ME ID stops being active (it checks every 6 s). Recorded on the node in `runs/devnet-killswitch-*.txt`:

1. The owner's ME ID is active, and the agent pays Acme. **Paid.** A second, larger invoice is held for approval.
2. The global DAO sends `MsgUpdateDidStatus` and the owner's ME ID becomes inactive.
3. In the next block, the same agent, vendor and limits: `whois` says `can pay: NO: ME ID … is not active`, and
   the payment is refused. The owner can't approve the held payment either (a suspended owner can stop money,
   never move it).
4. The ME ID is re-activated, and the same invoice goes through. The held payment can now be approved.
5. An owner with only real-person verification (KYC level 1) tries to deposit: `has KYC level 1, need 2`.
6. A batch to Acme plus an unlisted vendor that holds an ME ID settles through ME Hub's `multi_send`. **Paid.**

`npm run devnet` (`scripts/devnet-runs.sh`) rebuilds the node from scratch and records every devnet run:
the kill switch, Claude Code as the agent over MCP (`runs/devnet-claude-*.jsonl`), the obey-everything
client (`runs/devnet-obey-*.txt`) and agent-to-agent commerce (`runs/devnet-commerce-*.txt`).

## Develop

```bash
npm ci
npm test          # 39 contract tests (incl. a random-operation accounting test and garbage-input parser checks) + 33 JS tests (amounts, send outcomes incl. a halted chain, offline tx proofs, RPC failover, MCP, paywall, SSRF, CLI, fixed-deposit encoding)
npm run build     # reproducible wasm (needs Rust 1.81 + wasm32, binaryen, cosmwasm-check 1.5.8)
npm run judge     # verify + JS tests + run records + every linked testnet tx, read-only, one line each
npm run verify    # deployed code == artifacts/leash_id.wasm, and no admin can swap it
npm run devnet    # local ME Hub node: Claude over MCP, obey-everything, kill switch, commerce → runs/
node scripts/check-runs.mjs        # asserts what those records must show (CI re-runs all but the Claude run on a fresh node)
npm run check:chain                # fetches every testnet tx the records link to: outcome and reason must match
npm run check:chain:offline        # the same with no network, from runs/chain-snapshot.json (each tx proven into its signed block)
npm run docs & npm run e2e:owner   # drives the owner console in a real browser against that node
```

The canonical `artifacts/leash_id.wasm` is the x86_64 Linux build from CI; CI rebuilds it byte for byte.

Costs: a payment is about 260k gas, roughly 0.00036 MEC in fees at testnet prices; refusals cost nothing by default (caught in simulation; the recorded demos broadcast them on purpose so they can be looked up).
See [docs/GAS.md](docs/GAS.md).

Endpoints (testnet): RPC `https://beta-hub-26657.explorer-testnet.me`, REST `https://beta-hub-1317.explorer-testnet.me`,
explorer `https://www.explorer-testnet.me`.

## Status

Release 0.7.0 (contract 0.5.2), testnet, with payment flows recorded against a real testnet ME ID. Built for the MetaEarth Hackathon 2026 "Harbor" (AI & Trust Infrastructure). The design ideas
(agent spending leashes, attack replays, MCP-driven agents) come from our earlier project
[Invoice Leash](https://github.com/eltociear/invoice-leash), which ran on another chain. See
[CHANGELOG.md](CHANGELOG.md).

Apache-2.0 license.
