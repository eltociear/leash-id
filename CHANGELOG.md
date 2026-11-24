# Changelog

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
