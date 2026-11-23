# Threat model

Leash ID assumes **the agent will eventually do the wrong thing.** A prompt injection, a fake invoice, a
confused model or a stolen agent key all end up the same way: the agent's key signs a transaction it
shouldn't. So we don't try to make the agent trustworthy. We limit what its key can do, and we enforce
those limits in the ME Hub contract, not in the agent, the MCP server or the prompt.

## Actors and keys

| Actor | Holds | Trusted for |
|---|---|---|
| Owner | owner key + a live ME ID (KYC ≥ `min_kyc_level`) | setting the leash, approving held payments |
| Agent (LLM + MCP server) | agent key only | nothing; every action is checked on-chain |
| Vendor | an address, optionally an ME ID | nothing |
| KYC issuer / global DAO (ME Network) | x/did | creating, suspending and re-activating ME IDs |
| Leash ID contract | escrowed budget | the checks below; its code is public and reproducible |

The MCP server (`mcp/server.mjs`) holds **only the agent key**. On purpose, it also exposes tools the agent
must never be able to use: `approve_payment`, `withdraw_budget`, `raise_my_limit`, `send_mec`. If those calls
are refused, the refusal comes from the chain, so it doesn't depend on the tool list or the prompt.

## Threats

| # | Threat | Example | What stops it | Evidence |
|---|---|---|---|---|
| T1 | Prompt injection makes the agent pay an attacker | hidden "SYSTEM NOTICE" in `inbox/demo.json` m4 | vendor must be allow-listed or hold a live ME ID at KYC ≥ N | `vendor_rules`; obey run: `vendor … is not allowed by this leash` |
| T2 | Business-email compromise ("CEO wire") | m3: urgent 0.04 MEC to a new supplier | vendor rule + `per_tx_limit` | obey run |
| T3 | Duplicate invoice / "pay again to be safe" | m2 re-sends INV-1001 | `(owner, vendor, invoice)` is paid once; checked before limits so the answer is "already paid as #N" | `duplicate_invoice_is_refused` |
| T4 | Agent raises its own limits or approves its own held payment | m4 asks for `raise_my_limit`, `approve_payment` | `update_policy` and `approve` are owner-only | `approval_flow_reserves_funds`; obey run: `only the owner can do this` |
| T5 | Agent drains the escrow | `withdraw_budget`, `send_mec`, one big pay | `withdraw` is owner-only; the escrow sits in the contract, not in the agent's account | `agent_cannot_attach_funds_or_drain_via_withdraw` |
| T6 | Splitting a big amount into many small ones | 5 × 0.014 MEC in one batch | UTC-daily limit counts every item and every held payment; batches can't contain items that need approval | `limits_and_utc_day_reset`, `batch_uses_me_multi_send`; obey run |
| T7 | Agent key stolen | attacker signs `pay` directly, without the MCP server | same contract checks; worst case = what's left of today's limit, to allowed vendors only. Owner calls `revoke_agent` | `revoke_expiry_and_ownership` |
| T8 | Owner's identity goes bad (fraud, a stolen account, a lapsed KYC) | ME ID suspended by the global DAO | **every `pay` re-reads the owner's ME ID from x/did.** Suspended → the next block refuses every agent under it. A suspended owner can still **reject** held payments but cannot **approve** them | `suspending_the_owner_me_id_freezes_the_agent_immediately`, `suspended_owner_can_reject_but_not_approve`; `runs/devnet-killswitch-*.txt` |
| T9 | Counterparty can't tell who is behind an agent | vendor receives a payment from an unknown address | `whois(agent)` returns the owner and their live ME ID; every receipt stores both ME IDs as of payment time | verifier page `docs/` |
| T10 | Low-assurance owner | real-person check only (KYC level 1) | `min_kyc_level` (2 on our deployments) gates `deposit` and `register_agent`, and is re-checked on every `pay` | `low_kyc_owner_is_refused`; devnet run step 5 |
| T11 | Self-dealing | agent lists itself as vendor | `vendor == agent` is refused | `vendor_rules` |
| T12 | A forgotten agent | old key in an old server | every leash has a required `expires_at`; after it, every `pay` is refused | `revoke_expiry_and_ownership` |

## What Leash ID does *not* protect against

- **The owner key.** Whoever holds it can change the leash, approve, withdraw and revoke. Keep it off the
  agent's machine. A multisig owner is a natural next step; the contract only needs an address with an ME ID.
- **Payments the leash allows.** A legitimate-looking 0.012 MEC invoice from an allow-listed vendor will be
  paid. Leash ID limits how much can go wrong. Judging whether the invoice is right is still the model's job
  (see `runs/devnet-claude-*.jsonl`, where Claude declines m3 and m4 before the chain ever sees them).
- **"Verified vendor" means "a real, KYC'd person or business", not "honest".** If that's not enough, turn off
  `any_verified_vendor` and keep only the allowlist.
- **ME ID semantics.** We trust x/did: `DID_STATUS_ACTIVE` and `kycLevel` as returned by
  `/metaearth.did.Query/Did`. If the DID query fails or can't be parsed, the owner is treated as unverified
  and payments are refused (fail closed, `did::lookup` returns `None`).
- **Contract admin.** Our deployments set the deployer as wasm admin so we can fix bugs during the hackathon
  (we did, see `deployment.json` → `superseded`). A production deployment should clear the admin or hand it to
  a DAO. `scripts/verify.mjs` shows which code is deployed and whether it matches the source in this repo.
- **Liveness.** If ME Hub halts, nothing gets paid. That fails safe for the owner.

## Fail-closed choices

- Unknown agent → refused. Unknown DID response → unverified → refused.
- Held payments count against the daily limit and reserve budget the moment they're created.
- Agents never call `pay_batch` with an item that needs approval; the whole batch is refused instead.
- Refusals are real failed transactions (fixed gas, no simulation), so they're visible on the explorer.
