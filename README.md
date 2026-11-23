# Leash ID — KYC-backed AI agents on ME Network

![Leash ID: every agent payment re-checks the owner's live ME ID on ME Hub](assets/cover.png)

**Check an agent:** https://eltociear.github.io/leash-id/ (reads ME Hub testnet live, no wallet)

AI agents are starting to pay invoices and buy services. Two things are missing:

1. **Counterparties can't tell who stands behind an agent.** An address is just an address.
2. **Owners have no hard guarantee the agent can't overspend.** It's "hand over your keys" or "approve everything by hand."

Leash ID is a CosmWasm contract on **ME Hub** that ties every agent to a **live ME ID** (ME Network's native KYC identity) and enforces a spending leash on-chain.

```
            ┌────────────── ME Hub (mechain_400-1) ───────────────┐
 owner ───► │ Leash ID contract                                    │
 (ME ID,    │  register_agent / deposit / approve / revoke         │
  KYC ≥ L2) │                                                      │
            │  pay(vendor, amount, invoice)  ◄─────────── agent key │ ◄── Claude / any LLM (MCP)
            │   1. owner's ME ID still ACTIVE and KYC ≥ min? ──────┼──► x/did  (Stargate query
            │   2. vendor allow-listed, or vendor holds an ME ID? ─┼──► x/did   /metaearth.did.Query/Did)
            │   3. per-payment / UTC-daily limits, budget, expiry  │
            │   4. invoice never paid before (owner, vendor, ref)  │
            │   5. over threshold → pending until owner approves   │
            │  pay_batch → ME Hub `multi_send` custom message      │
            │  whois(agent) → owner + live ME ID + limits          │ ◄── any counterparty
            └──────────────────────────────────────────────────────┘
```

## Why this is ME-native

- **On-chain identity checks, no oracle.** ME Hub whitelists one identity query for CosmWasm: `/metaearth.did.Query/Did` (`me-hub/app/keepers/wasm_me_query.go`). Leash ID calls it inside `register_agent`, `deposit`, **every `pay`**, and `whois`. If an owner's ME ID is suspended, every agent under it stops paying in the next block. Nobody has to revoke anything.
- **KYC-aware vendor rules.** A leash can allow "any vendor that holds a live ME ID at KYC level ≥ N", so agents can pay verified merchants without a hand-kept allowlist.
- **ME Hub's custom `multi_send` message** (`wasm_me_msg.go`) settles batched payouts in one bank message.
- **Receipts carry identities.** Every payment stores the owner's ME ID number and the vendor's ME ID (if any) as of the moment it was paid.

## Contract

`contracts/leash-id` (Rust, cosmwasm-std 1.5, built with Rust 1.81 for wasmvm 1.4).

| Execute | Who | What |
|---|---|---|
| `deposit {}` | owner (ME ID required) | escrow the agent budget |
| `withdraw {amount}` | owner | take back unreserved budget |
| `register_agent {agent, policy}` | owner (ME ID required) | leash an agent key |
| `update_policy` / `revoke_agent` | owner | change or cut the leash |
| `pay {vendor, amount, invoice, memo}` | agent | pay within the leash |
| `pay_batch {payments}` | agent | up to 20 payments via ME `multi_send` |
| `approve {id}` / `reject {id}` | owner | settle payments above `approve_above` |

| Query | |
|---|---|
| `whois {agent}` | owner, live ME ID, `verified`, limits, spent today, remaining budget |
| `check_pay {agent, vendor, amount, invoice}` | dry-run; runs the same `evaluate()` as `pay` |
| `owner`, `payment`, `payments`, `config`, `me_id` | read state |

Policy: `per_tx_limit`, `daily_limit` (UTC day), `approve_above`, `vendors`, `any_verified_vendor`, `expires_at`.

## Run

```bash
npm install
npm test                      # 17 contract unit tests (fake DID module behind the Stargate query)
npm run build                 # reproducible wasm + cosmwasm-check with ME Hub's capabilities
node scripts/keygen.mjs       # testnet-only keys in ~/.config/leash-id/keys.env
node leash.mjs addrs          # balances + ME ID of each role
node leash.mjs deploy         # store + instantiate on mechain_400-1
node leash.mjs deposit 10
node leash.mjs register agent --per-tx 2 --daily 5 --approve-above 1.5 --vendors vendor
node leash.mjs pay vendor 1 INV-001          # as the agent
node leash.mjs whois agent
```

Endpoints (testnet): RPC `https://beta-hub-26657.explorer-testnet.me`, REST `https://beta-hub-1317.explorer-testnet.me`, explorer `https://www.explorer-testnet.me`.

## Status

Built for the MetaEarth Hackathon 2026 "Harbor" (AI & Trust Infrastructure). This is a new codebase. The design ideas (agent spending leashes, attack replays, MCP-driven agents) come from our earlier project [Invoice Leash](https://github.com/eltociear/invoice-leash), which ran on another chain.

Apache-2.0 license.
