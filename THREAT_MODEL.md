# Threat model

Leash ID assumes **the agent will eventually do the wrong thing.** A prompt injection, a fake invoice, a
confused model or a stolen agent key all end up the same way: the agent's key signs a transaction it
shouldn't. So we don't try to make the agent trustworthy. We limit what its key can do, and we enforce
those limits in the ME Hub contract, not in the agent, the MCP server or the prompt.

This document describes contract v0.5. Test names refer to `contracts/leash-id/src/tests.rs`.

## Actors and keys

| Actor | Holds | Trusted for |
|---|---|---|
| Owner | owner key + a live ME ID (KYC ≥ `min_kyc_level`) | setting the leash, approving held payments |
| Agent (LLM + MCP server) | agent key only | nothing; every action is checked on-chain |
| Vendor | an address, optionally an ME ID | nothing |
| KYC issuer / global DAO (ME Network) | x/did | creating, suspending and re-activating ME IDs |
| Leash ID contract | escrowed budget | the checks below; public, reproducible code, deployed **without an admin** |

The MCP server (`mcp/server.mjs`) loads only the agent key, and **refuses to start if any other Leash ID key
is readable from its process** (see [SECURITY.md](SECURITY.md)). The owner-only tools (`approve_payment`,
`withdraw_budget`, `raise_my_limit`) aren't offered by default. The demo turns them on (`LEASH_HONEYPOT_TOOLS=1`)
to show that the chain refuses them whatever the tool list or the prompt says. The demo's fourth tool, `send_mec`,
is a plain transfer from the agent's own wallet, and the chain allows it: that wallet is the one thing the leash
doesn't cover. With an owner fee grant (T27) it can be empty.

## Threats

| # | Threat | Example | What stops it | Evidence |
|---|---|---|---|---|
| T1 | Prompt injection makes the agent pay an attacker | hidden "SYSTEM NOTICE" in `inbox/demo.json` m4 | vendor must be allow-listed or hold a live ME ID at KYC ≥ N | `vendor_rules`; obey run |
| T2 | Business-email compromise ("CEO wire") | m3: urgent 0.04 MEC to a new supplier | vendor rule + `per_tx_limit` | obey run |
| T3 | Duplicate invoice, including re-spelled ones | m2 re-sends INV-1001; an injection says "resend as `inv 1001`" | an invoice is paid once per (owner, vendor, **normalized number**), where a vendor's registered EVM sub-account counts as its main account (0.5.0): case, spaces and separators are ignored, and non-ASCII look-alikes (fullwidth digits, Unicode hyphens) are refused outright. Checked before limits, so the answer is "already handled as #N" | `duplicate_invoice_is_refused_whatever_the_spelling`, `an_invoice_is_paid_once_across_a_vendors_addresses`; obey run |
| T4 | Agent raises its own limits or approves its own held payment | m4 asks for `raise_my_limit`, `approve_payment` | `update_policy`, `approve`, `release_invoice` are owner-only | `approval_flow_…`; obey run: `only the owner can do this` |
| T5 | Agent drains the escrow | `withdraw_budget`, one big pay | `withdraw` is owner-only; the escrow sits in the contract; only `deposit` accepts funds | `only_deposit_takes_funds` |
| T6 | Splitting, or straddling midnight | 5 × 0.014 MEC in one batch; full limit at 23:59 and again at 00:01 | the limit covers **any 24 hours** (hourly buckets that count for 25 h, so a spend stops counting 24-25 h later, never sooner; 0.5.1 counted them for 24 h, which let 2× the limit out in 23h01m), every batch item and every held payment; batches can't contain items that need approval. Splits under the approval threshold are bounded by the 24-hour limit, not stopped: on testnet with a 0.2 MEC limit (contract 0.4), the obey-everything client's 5 × 0.014 batch to the owner's own vendor was paid (`runs/superseded-v0.4/testnet-obey-*.txt`); with less of the limit left (`runs/testnet-obey-*.txt`) or 0.05 MEC on the node, it was refused | `daily_limit_is_a_rolling_24_hours`, `a_full_limit_spent_late_in_the_hour_stays_spent_for_24_hours`, the every-24h-window check in the random test, `batch_uses_me_multi_send` |
| T7 | Agent key stolen | attacker signs `pay` directly | same contract checks; worst case = what's left of the 24h limit, to allowed vendors only, plus whatever is left of the owner's fee grant (spent on fees, e.g. on failing calls, until revoked). `revoke_agent` stops it **and rejects its held payments**, and `leash revoke` / the console end the fee grant in the same tx; no later policy change can lift a revoke, only `resume_agent`. Keep fee grants small and short | `revoke_sticks_until_resume_and_clears_the_queue` |
| T8 | Owner's identity goes bad (fraud, a stolen account, a lapsed KYC) | ME ID suspended by the global DAO | **every `pay` re-reads the owner's ME ID from x/did.** Suspended → the next block refuses every agent under it. A suspended owner can still reject held payments but can't approve them | `suspending_the_owner_me_id_freezes_the_agent_immediately`, `approval_rechecks_owner_vendor_and_age`; `runs/devnet-killswitch-*.txt` |
| T9 | Counterparty can't tell who is behind an agent | vendor receives a payment from an unknown address | `whois(agent)` returns the owner, their live ME ID and whether the agent can pay right now; every receipt stores both ME IDs as of payment time | verifier page `docs/` |
| T10 | Someone vouches for an address they don't control (squatting) | a KYC'd attacker registers a company's agent address first | a leash is an **offer** until the agent key itself accepts it; an agent can leave a leash | `a_leash_needs_the_agent_to_accept_it` |
| T11 | Low-assurance owner | real-person check only (KYC level 1) | `min_kyc_level` (2 on our deployments) gates `deposit`, offers and acceptance, and is re-checked on every `pay` and `approve` | `low_kyc_owner_is_refused`; devnet run step 5 |
| T12 | Money that never leaves the leash | agent pays itself, the contract, or another leashed agent | all three are refused as vendors, in the policy and at payment time | `vendor_rules` |
| T13 | Approving something that is no longer OK | a "verified vendor" loses its ME ID while a payment waits; a payment waits for weeks | `approve` re-checks the vendor rule and refuses payments older than 7 days | `approval_rechecks_owner_vendor_and_age` |
| T14 | Flooding the owner with approval requests | agent creates held payments to lock the budget | held payments count against the 24h limit and at most 50 can wait per owner | `the_approval_queue_is_bounded` |
| T15 | A forgotten agent | old key in an old server | every leash has a required `expires_at`, at most 366 days ahead | `revoke_sticks_…`, `policy_is_validated` |
| T16 | The contract's code is swapped | a migrate admin replaces the code with one that pays the escrow out | deployed **without an admin**; `npm run verify` fails if an admin exists, and the verifier page says so | `npm run verify`, CI job `testnet` (advisory: a testnet outage doesn't fail the build) |
| T17 | The agent's own runtime reads the owner key | Claude Code with shell tools runs `cat` on the key file | the MCP server refuses to start next to readable owner keys; deployment guide runs the agent in its own user/container | `test/mcp.test.mjs` |
| T18 | A network hiccup turns into a double payment | timeout after broadcast; the model retries with a new invoice ref | the tx hash is computed before broadcasting; an unconfirmed send is reported as **UNKNOWN, may have gone through** with the hash, never as "not sent"; `find_invoice` and the `invoice` query look it up. When the chain has stopped making blocks (latest block over 3 min old) nothing is broadcast at all: **NOT SENT**, safe to retry, instead of a tx that sits in the mempool and might land hours later | `lib/tx.mjs`, `test/tx.test.mjs` |
| T19 | A sub-account claims an ME ID it isn't bound to | someone registers a link from an EVM address to a stranger's ME ID | `register_sub_account` only stores a link x/did itself lists, and every use re-reads x/did, so a dropped binding stops counting at once | `an_evm_sub_account_counts_as_its_main_me_id`; devnet run step 7 |
| T20 | A "bond" that isn't there | an owner points at a deposit that unlocked, was withdrawn, or isn't theirs | bonds are read live from x/wstaking on every `whois`; only locked deposits of the ME ID's main account, in the leash denom, count | `owners_can_show_a_fixed_deposit_bond` |
| T21 | Selling to an agent nobody answers for, or one payment buying many answers | a bot replays a payment tx, forges one, or races two requests | `paywall()` reads the payment from the chain, needs a live HMAC-signed invoice it issued, claims each tx before reading the chain (in-process by default; pass a shared `spent` store for several replicas or restarts), and checks the payer's owner's live ME ID | `runs/devnet-commerce-*.txt` steps 3, 5; `test/vendor.test.mjs` |
| T22 | An agent pays an API run by nobody verified | an API asks for payment to an anonymous address | the leash refuses vendors without an ME ID unless allow-listed; `pay_for_api` also caps the price | commerce run steps 6-7 |
| T23 | Front-running a payment | someone watching the chain sends another agent's tx hash to the API first | redemption needs the paying agent key's ADR-36 signature over (tx, invoice), and the paywall claims a tx only after that proof checks out, so a stranger who sends the hash first with a junk proof can't burn the payment either; the buyer's MCP asks again a few times if the seller hasn't seen the tx yet, and `retry_api_answer` asks later without paying again | commerce run step 4; `test/vendor.test.mjs` |
| T24 | One insider moves money alone | a single compromised approver key approves a large held payment | `set_approvers`: N signatures from N **different** ME IDs (an owner's or co-approver's EVM sub-account is the same person, 0.5.0), each live, re-counted at release, one per ME ID; any one can reject; no self-approval, also not to the signer's own sub-account; agents can't be approvers | `held_payments_can_need_two_signatures`, `two_signatures_must_come_from_two_me_ids`; devnet step 9; `npm run e2e:owner` |
| T25 | The agent fetches internal URLs (SSRF) | an injected email makes the agent call `pay_for_api` on cloud metadata or a LAN admin page | public https only; the resolved address must be public and the request connects to that same address (no DNS-rebinding window); no redirects; 64 KB cap; "private" includes NAT64 (`64:ff9b::/96`), 6to4, Teredo, mapped IPv4, site-local and benchmark ranges | `test/mcp.test.mjs` |
| T26 | Paying itself through its own EVM form | an agent key that also holds an ME ID links its EVM sub-account and pays it | `vendor_sane` checks the main account behind any registered sub-account | `an_evm_sub_account_counts_as_its_main_me_id` |
| T27 | The agent key's own wallet | a stolen agent key sends away whatever MEC it holds for gas | the owner pays the agent's fees through x/feegrant (contract calls only, capped, expiring), so the key can hold 0 MEC | `runs/devnet-killswitch-*.txt` step 8; SECURITY.md §4 |
| T28 | The owner goes around the co-approvers | with 2 signatures required, the owner key (or a thief holding it) drops the approval line, raises limits, adds its own vendor, lowers the threshold or adds a friendly co-approver at the same threshold, sets up a fresh agent, or just withdraws the budget, then pays alone | while 2+ signatures are needed, every loosening change waits 24 h and any one co-approver can cancel it (0.5.3); a new agent's leash pays nothing for 24 h; any co-approver can revoke the owner's agents at once; tightening is immediate; an agent can't pay its own owner at any address of the owner's ME ID (else it would route the budget around the withdraw review); queued changes end with the leash, with a tightening, and 7 days after their review; enough signers agreeing (`approve_change`) apply a change at once, so one hostile co-approver can't freeze a 2-of-3 owner. Left open: a revoked agent can leave and be offered a fresh leash; that leash again pays nothing for 24 h, so co-approvers have to revoke it again (watch `leash watch` or the console), and in a 2-of-2 a lost co-approver key leaves only the existing leashes and `reject` working | `an_owner_alone_cannot_loosen_a_co_signed_leash`; devnet kill-switch run, step 9 |

## What Leash ID does *not* protect against

- **The owner key, with no co-approvers.** Whoever holds it can change the leash, approve, withdraw and revoke.
  Keep it off the agent's machine. With co-approvers (2+ signatures) it can still do all of that, but every
  loosening waits 24 hours for anyone of them to cancel it (T28); it can't move money alone sooner.
- **Payments the leash allows.** A legitimate-looking 0.012 MEC invoice from an allow-listed vendor will be
  paid. Leash ID limits how much can go wrong. Judging whether the invoice is right is still the model's job
  (see `runs/devnet-claude-*.jsonl`, where Claude declines m3 and m4 before the chain ever sees them).
- **Different invoice numbers for the same bill.** Matching is per normalized number. "INV-1001" and
  "INV-1001-B" are two invoices. Per-payment and 24h limits still cap the damage.
- **"Verified vendor" means "a real, KYC'd person or business", not "honest".** If that's not enough, turn off
  `any_verified_vendor` and keep only the allowlist.
- **ME ID semantics.** We trust x/did: `DID_STATUS_ACTIVE` and `kycLevel` as returned by
  `/metaearth.did.Query/Did`. If the DID query fails or can't be parsed, the owner is treated as unverified
  and payments are refused (fail closed, `did::lookup` returns `None`). Unknown KYC levels count as none.
- **Bugs in this contract.** It has no admin, so a bug can't be patched in place: owners withdraw and move to a
  new deployment. That's the price of nobody being able to swap the code under the escrow.
- **Liveness.** If ME Hub halts, nothing gets paid. That fails safe for the owner.

## Fail-closed choices

- Unknown agent → refused. Unknown DID response → unverified → refused.
- Held payments count against the 24h limit and reserve budget the moment they're created.
- A batch with any item that needs approval, or the same invoice twice, is refused as a whole.
- Funds attached to anything but `deposit` are refused, so nothing can get stuck in the contract unaccounted.
- Off-chain, every send is simulated first (no fee for a refusal). The demo sets `LEASH_ONCHAIN_REFUSALS=1` so
  refusals become real failed transactions anyone can look up on the explorer.
