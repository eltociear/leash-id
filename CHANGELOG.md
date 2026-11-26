# Changelog

## Unreleased

- `leash watch` says when the chain stops ("ME Hub isn't producing blocks … nothing can be paid, approved or revoked
  until it moves") and when it's back, once each, to the console and the webhook. Before, a halt looked like a
  quiet day.

- **Fixes from an independent review of the off-chain code** (no high-severity findings; all of these are fixed):
  - Paywall: a tx is claimed only after the payer's proof checks out. Before, anyone who saw the payment on chain could
    send its hash first with a junk proof and burn it, so the real buyer paid and got "already used".
  - MCP `pay_for_api`: if the seller hasn't seen the tx yet (402/503), it asks again a few times; the new
    `retry_api_answer` tool asks later without paying again. After an UNKNOWN payment it names the invoice for
    `find_invoice` and won't call that seller again until it's settled (a second call would get a fresh invoice that
    the contract's invoice dedupe can't match).
  - MCP key check also refuses keys of other roles in its environment (`LEASH_OWNER_MNEMONIC`…), which a model's
    shell inherits. New test.
  - Vendor-supplied invoice references must be plain characters and are quoted in headlines; `action_log` is framed
    as untrusted (its arguments came from emails and APIs).
  - SSRF guard: NAT64, 6to4, Teredo, site-local, hex-mapped IPv4, 192.0.0.0/24 and 198.18.0.0/15 count as private.
  - "tx already in mempool" (sdk code 19) is UNKNOWN, not NOT SENT: those bytes did reach a node.
  - Sandbox: control payments at least 2 minutes apart and one at a time per instance, closing the seconds before a
    payment shows on chain in which two instances could both see room under the daily cap.
  - The `send_mec` honeypot no longer sends: it refuses, so a hijacked model can't drain the agent's gas money.
  29 JS tests.

- Link previews: every page (verifier, owner console, about, tx, sandbox) has Open Graph / Twitter tags and a
  1120×630 card (`docs/og.jpg`), so a link pasted in Telegram, X or Discord shows the project, not a bare URL.

- Sends check the chain is moving before broadcasting. When the latest block is over 3 minutes old
  (`LEASH_MAX_BLOCK_AGE_S`, 0 turns it off), the CLI, the MCP server and the sandbox answer NOT SENT, "ME Hub isn't
  producing blocks (the latest, #5523300, is 7.8 h old) … safe to retry", in under a second. Before, the tx went into
  a halted chain's mempool, the caller waited 60 s for UNKNOWN, and the payment could land hours later. Checked
  against the halted testnet; new test in `test/tx.test.mjs` (27 JS tests). `leash whois` and the MCP `whoami` and `verify_agent` say the same when the
  contract would let an agent pay but the chain can't carry it ("can pay: yes by its leash, but ME Hub isn't producing
  blocks …"; `chain_halted: true` in the structured result), so another agent doesn't read "yes" as "it will land".

- `npm audit`: 0 vulnerabilities (puppeteer-core 25; the advisories were in its FTP/proxy dependencies, e2e only).

- The hosted sandbox's nav linked to `/docs/…`, which Vercel doesn't serve (404): it now links to the Pages site,
  and every Pages page links to the sandbox ("Red-team it"). Sandbox tx links open the readable tx page.
- The sandbox's control-payment label says when it's only simulated (chain halted); `/api/state` gives the policy
  in MEC only (it mixed umec and MEC). `npm run judge` starts with the latest testnet block and its age.
- JUDGES: the hosted page has no Claude (said plainly, with the recorded run linked); who it's for and how it
  would reach mainnet; x/feegrant moved from "ME depth" to product (it's a stock Cosmos module); the bond line
  no longer says testnet has no fixed-deposit config (it does: `me_earth`, 1 day).

- **The red-team sandbox is hosted**: https://leash-id-sandbox.vercel.app/ (the testnet sandbox agent's key, fees on
  the owner's grant). Vercel's loader can't `require()` an ES module, which `@cosmjs/encoding` does with
  `@scure/base` 2.x, so the functions are bundled with esbuild at deploy (`scripts/bundle-api.mjs`); local runs and CI
  are unchanged. It stops waiting for inclusion at 30 s (functions are cut at 60 s) and says why a send didn't happen.
- **Every page shows the latest block and its age**, and says plainly when the chain isn't producing blocks.
- JUDGES opens with a five-line summary.
- **`leash lock <MEC>` makes a real bond**: it sends ME Hub's own `MsgDoFixedDeposit` (x/wstaking; cosmjs has no
  type for it, so `lib/wstaking.mjs` encodes it from me-hub's tx.proto), reads the new deposit's id from the
  `fixed_deposit` event, then `set_bond`s it next to any deposits already shown. It checks the owner's KYC region
  offers the term first (on testnet only `me_earth`, 1 day, 0.01 MEC minimum). Checked by simulation on testnet;
  encoding pinned byte for byte in `test/wstaking.test.mjs`.
- README.ja listed 35 contract tests; there are 39.

## 0.7.0 (contract 0.5.2, redeployed on testnet: code 46, `me1dsl645w…vwsrnp`, no admin)

- **The daily limit now holds over any 24 hours, to the second.** Spend is kept in hourly buckets, and a bucket
  used to count for 24 hours. So a full limit spent at 10:59 stopped counting at 10:00 the next day, and twice the
  limit could go out in 23h01m. A bucket now counts for 25 hours: a spend stops counting 24–25 hours later, never
  sooner. Found by a fresh-eyes review. Test `a_full_limit_spent_late_in_the_hour_stays_spent_for_24_hours`.
- The random-operation test now moves the clock by odd seconds, not whole hours, and checks the limit over every
  24-hour window of every agent's payments. Its harness also undoes a failed call the way the chain does. Mock
  storage had kept the first item of a refused batch, and the new check is what caught that.
- **The red-team sandbox runs with no key.** With no agent key on the machine, `node scripts/sandbox-dev.mjs`
  simulates every action as the live testnet sandbox agent (`simulateAs` in `lib/tx.mjs` uses the public key the
  account already published on chain), so anyone can attack it and get the deployed contract's own answers, with
  nothing broadcast. On the new contract: control payment would pass, all 8 attacks refused.
- **Readable transactions**: `docs/tx.html#<hash>` shows what a tx asked the contract and what it answered (the
  refusal reason in MEC, the payment it recorded, the ME IDs, the money it sent). JUDGES/README and the owner
  console link there instead of raw REST JSON; the raw answer is linked at the bottom.
- JUDGES: try the key-less MCP config with your own Claude (`claude --mcp-config mcp/judge.mcp.json`); `npm run
  verify` is described exactly (chain == committed wasm, which CI rebuilds and compares).
- README: the vendor KYC floor is the contract's `min_kyc_level`, not per leash (the old wording said "≥ N").
- CI checks that the owner console and the tx page point at the deployed contract too.
- The 0.5.1 testnet records are kept in `runs/superseded-v0.5.1/`.
- **Owner console, read-only**: open any owner's console without a wallet (`owner.html#view=<owner>`), e.g. our
  testnet owner. Same live queries; every control that signs is disabled. Checked in the e2e run.
- The sandbox page escapes an unexpected status and links only http(s) tx URLs (an injection review of all three
  pages; the verifier and the owner console already escaped every string that comes from the chain).

## 0.6.1 (contract 0.5.1, redeployed on testnet: code 44, `me1rmqthd…4066gc`, no admin)

A second adversarial review, of 0.5.0 itself, found that the 0.5.0 fix was incomplete:

- **An invoice paid at a sub-account before its link was recorded could still be paid at the main account.**
  0.5.0 only looked for that older entry when paying the sub-account again. Now a lookup checks every address of
  the vendor: the address itself, its main account, and that main account's registered sub-account. Test
  `a_link_recorded_later_still_catches_the_duplicate_and_links_dont_move`.
- **Sub-account links can't be moved.** If x/did moves a sub-account to another ME ID, the old link stays (and stops
  resolving, so it fails closed); re-pointing it would have let invoices filed under the old main be paid again.
- **Freeing an invoice removes exactly the entry the payment took**: payments record `invoice_party`.
- **A held payment whose signatures stand again can be released.** If a co-approver's ME ID lapsed after signing and
  came back, every signer got "already approved" and the payment waited until it expired. Now any signer can
  release it once enough signatures stand. Test `a_payment_whose_signatures_stand_again_can_be_released`.
- `leash revoke` no longer treats an unreadable fee grant as "none": it revokes the agent and tells you to run
  `gas-revoke`.
- The random-operation test now pays one vendor at two addresses, lapses and restores a co-approver's ME ID, and
  checks that no invoice is handled twice unless the owner released it.
- `deployment.json` records the contract's own version, and `npm run verify` checks it against the chain.
- The 0.5.0 testnet records are kept in `runs/superseded-v0.5.0/`.

## 0.6.0 (contract 0.5.0, redeployed on testnet: code 43, `me1aqk3ej…3hjq79`, no admin)

Found by an adversarial review of 0.5, fixed in the contract and recorded again on testnet:

- **An invoice is filed under the vendor's main account.** 0.4 keyed duplicates on the raw vendor address, so an
  invoice paid at a vendor's main account could be paid again at its registered EVM sub-account (and the other way
  round). Now one vendor, all of its addresses, one invoice. Test `an_invoice_is_paid_once_across_a_vendors_addresses`;
  on testnet in `runs/testnet-payments-*.txt` step 4.
- **Two-person approval needs two ME IDs.** 0.4 let an owner name their own EVM sub-account as the co-approver,
  which resolves to the owner's ME ID: one person met a 2-of-2. Now `set_approvers` refuses two addresses of one
  ME ID, `approve` counts one signature per ME ID, and a signer can't approve a payment to their own sub-account.
  Test `two_signatures_must_come_from_two_me_ids`; on testnet in step 5b.
- **Revoking an agent ends its fee grant in the same tx** (`leash revoke`, the owner console; `--keep-gas` to keep
  it), so a stolen key can't keep burning the owner's grant on failing calls. THREAT_MODEL T7 now counts the grant.
- `pay_for_api` checks that the seller's ME ID is live and at the contract's KYC level (not just present), and
  won't start a purchase whose price would be held for approval (the seller couldn't answer in time).
- The paywall takes a shared `spent` store (`claim`/`release`) for several replicas or restarts.
- Reads, simulations, signing and the paywall's tx lookup retry dropped connections (never a contract's answer),
  so a flaky public endpoint doesn't turn into "not sent".
- The owner console counts only signatures that still stand and disables Approve once you signed.
- The 0.4 testnet records are kept in `runs/superseded-v0.4/`.

- **"Before the owner signs"**: `leash whois`, the verifier and the owner console show the most an agent can
  still move with nobody's signature (what's left of its 24-hour limit, out of the owner's free budget) and the
  largest single payment that goes through unsigned. Below the approval threshold, that is the exposure.
- The obey-everything client is recorded on testnet too (`runs/testnet-obey-*.txt`), checked by `check-runs`.
- `npm run check:chain` (and the `testnet` CI job): every testnet tx the committed records link to is fetched
  from the chain; its outcome, the contract's reason for a refusal, and the contract called must match the record.
- The verifier and the owner console repeat a read up to twice when the public endpoint drops it or answers 5xx.

## 0.5.0 (agent side; contract unchanged, still 0.4.0 on testnet)

- **The agent key can hold 0 MEC.** `leash gas-grant <agent> --limit <MEC>` gives the agent an x/feegrant
  allowance from the owner that covers contract calls only (capped, expiring); `--fee-granter` / the MCP server
  (automatically) use it. A key that never held MEC accepts its leash and pays, and can't move anything else.
  Recorded on a real node (kill-switch run, step 9) and checked by `scripts/check-runs.mjs`.
- The owner console can pay an agent's fees (cap and days) and stop paying them; the verifier shows who pays an
  agent's fees and what its key holds outside the leash. Both are driven in Chrome by `npm run e2e:owner` in CI.
- **Testnet payment flows with a real ME ID**: our owner's testnet ME ID was issued (KYC 2), and
  `scripts/testnet-pay.sh` records every flow against it in `runs/testnet-payments-2026-11-25T05-22-54Z.txt`, checked by `scripts/check-runs.mjs`.
- RPC failover: `ME_RPC` takes a comma-separated list.
- About page; a 20-item batch measured at 1.41M gas; a garbage-input test for the DID, deposit and invoice parsers.
- Claims audit: every overstated or stale statement in the docs corrected (`send_mec`, what CI re-runs, sandbox
  status, key-isolation scope); `leash --json` exits 3 on an unknown outcome; `pay_for_api` connects to the
  address it checked (no DNS rebinding); send-path tests with a fake client.

## 0.4.0

From a second adversarial review (of 0.3), plus what a finance team asks for first.

- **Two-person approval.** `set_approvers {approvers, threshold}`: up to 5 co-approvers, each with a live ME ID;
  a held payment is released by `threshold` signatures (the owner's included), any one signer can reject.
  Signatures are re-counted at release: a co-approver whose ME ID was suspended no longer counts. An agent key
  can't be a co-approver, a co-approver key can't become an agent, and nobody can approve a payment to
  themselves. Owner console: progress per payment, a "waiting for your co-approval" queue, settings.
- **Fixed:** an agent whose key also held an ME ID could pay itself through its registered EVM sub-account.
  `vendor_sane` now applies to the main account behind a sub-account too.
- **Fixed (vendor paywall):** a tx hash is public, so anyone watching the chain could redeem another agent's
  payment. Redemption now needs the paying agent key's ADR-36 signature over the tx and invoice. Two concurrent
  requests can no longer redeem one payment. Invoices are HMAC-signed with an expiry (no per-request state).
- **Fixed (`pay_for_api`):** public https URLs only (no localhost, LAN, link-local or metadata addresses, no
  redirects); 64 KB response cap; the seller must name our contract and hold an ME ID (or be allow-listed); a
  seller that took payment and didn't deliver isn't paid again; only host and path go into the on-chain memo.
- **Fixed (sandbox):** only the fixed control payment is broadcast (daily cap); everything else is answered in
  simulation. Claude runs are capped (3 rounds, 3 tool calls, no parallel calls, 1024 tokens); budgets can be
  shared through Upstash; same-origin JSON only; generic errors; `.vercelignore`.
- **Accounting:** CSV export (`leash history --csv`, owner console), with spreadsheet formulas defused.
- **Gas table** (`docs/GAS.md`, `scripts/gas-report.mjs`): a payment is about 260k gas.
- `scripts/check-runs.mjs` now demands the exact set of payments that may go through and checks the specific
  refusal reason of each kind of attack. The devnet suite records the two-signature flow and the front-running attempt.

## 0.3.0

Deeper use of ME Network, the vendor side, and the whole system tested on a real ME Hub node in CI.

- **EVM sub-accounts.** `register_sub_account {sub, main}` (anyone may call it) records an EVM-form sub-account,
  but only if the main account's ME ID lists it in x/did, and the link is re-checked on every use. A merchant who
  is paid at its EVM address now counts as "holds an ME ID". Shown on a real node: refused, linked, paid.
- **Fixed-deposit bonds.** `set_bond {deposit_ids}`: an owner shows ME Hub fixed deposits (x/wstaking, the
  second of the three Stargate queries ME Hub opens to CosmWasm) as a bond. `whois` reads them live; deposits that
  unlocked or were withdrawn drop out. The verifier page and owner console show it.
- **`identity {address}`** query: ME ID (direct or via its main account) and bond for any address.
- **Agent-to-agent commerce over HTTP 402.** `leash-id/vendor`: `paywall()` and `verifyPayer()` let an API sell
  to AI agents only when a verified human answers for them (payment read from the chain, one response per tx,
  invoices issued per request). MCP tool `pay_for_api`: checks the seller's ME ID, pays through the leash within
  a price cap, retries with the proof. `examples/paid-api` serves real chain data.
- **Red-team sandbox** (`sandbox/`, `api/`): a real agent key anyone can attack from a web page, including an
  email that Claude reads and acts on; the contract has the last word.
- **Tests.** A random-operation test of the accounting invariants (40 seeds × 250 operations, every path
  exercised). `scripts/check-runs.mjs` asserts the outcomes in the run records (not Claude's own choices). CI now builds me-hub and runs the
  devnet suite (attack replay, kill switch, sub-account, commerce) and the owner-console browser test on every
  push. The build fails on any float instruction (ME Hub's wasmvm refuses them at upload).
- Region rules were evaluated and dropped: x/did's `regionId` is marked unused in me-hub, so it can't be relied on.

## 0.2.0

An adversarial review of 0.1 found ways around the leash. All of them are fixed, and each has a test.

### Contract

- **A revoke now sticks.** `update_policy` used to clear `revoked`, so tightening a stolen key's limits
  re-enabled it. Only the new `resume_agent` lifts a revoke. Revoking also rejects the agent's held payments.
- **Duplicate invoices can't be re-spelled.** Invoice numbers are matched ignoring case, spaces and separators
  (`INV-1001` = `inv 1001`), and non-ASCII look-alikes are refused.
- **The daily limit is a rolling 24 hours**, so spending the full limit before and after UTC midnight no longer
  doubles it.
- **Leashes need the agent's consent.** `register_agent` is now an offer; the agent key calls `accept_leash`.
  Nobody can vouch for, or squat, an address they don't control. Agents can `leave_leash`.
- **Rejecting a held payment frees its invoice number**, and the owner can `release_invoice` a handled one.
- **Approving re-checks the vendor** (a "verified vendor" may have lost its ME ID) and refuses payments older
  than 7 days. At most 50 payments can wait per owner.
- **Only `deposit` accepts funds**; coins sent with anything else are refused instead of getting stuck.
- Vendors can't be the contract or another leashed agent. `approve_above` must be below `per_tx_limit`;
  `expires_at` at most 366 days ahead. Limits can't overflow into a crash.
- New queries: `payments_by_agent`, `payments_by_owner` (newest first), `pending`, `invoice`, `offers`,
  `version`. `whois` adds `can_pay_now`, `blocked_by`, `pending_count`, `min_kyc_level`.
- A `leash_payment` event for every payment created, approved or rejected. cw2-compatible version info and a
  `migrate` entry point. Deployed **without an admin**.
- Unknown KYC levels from x/did count as none.

### Owner side

- **Owner console** (`docs/owner.html`): connect Keplr; approve or reject held payments, revoke and resume
  agents, change limits and vendors, offer new leashes, deposit and withdraw. Tested end to end in a real browser
  against a local ME Hub node (`npm run e2e:owner`). The signing library is bundled and reproducible in CI.
- **`leash watch`**: follows payments as they happen and pushes the ones that need approval to a Slack-compatible
  webhook.

### Agent side

- The MCP server loads only the agent key and refuses to start while other keys are readable from its process.
  Keys are one file per role (`leash keygen --split` migrates the old `keys.env`).
- Sends compute the tx hash before broadcasting. A send that isn't confirmed is `UNKNOWN` (with the hash),
  never "not sent", so a retry can't double-pay. New `find_invoice` tool and `leash invoice` command.
- Sends are simulated first (refusals cost no fee), serialized per key, and classify wallet problems
  (no gas money, sequence) separately from contract refusals.
- MCP results: `PAID`, `HELD FOR OWNER APPROVAL`, `REFUSED`, `UNKNOWN`, `NOT SENT`, with `isError` and
  `structuredContent`. Inbox content is framed as untrusted. Owner-only "honeypot" tools are off unless
  `LEASH_HONEYPOT_TOOLS=1`. Logs go to `~/.local/state/leash-id` and can't break a payment.
- CLI rewritten: `parseArgs`, per-command help, exit codes, `policy` changes only the flags you pass, new
  commands for the two-step leash, approvals, history and invoices. Installable: `leash`, `leash-mcp`.
- Verifier page: tells "not registered" from "couldn't reach ME Hub", shows whether the agent can pay right
  now and whether the contract has an admin, pages through the agent's own history, works on phones and with
  a keyboard or screen reader.
- `npm run verify` fails when the contract has an admin. CI adds clippy, JS tests, npm audit and pinned actions.

## 0.1.0

Hackathon version: ME ID check on every payment, vendor rules, limits, approvals, multi_send batches, MCP server.
