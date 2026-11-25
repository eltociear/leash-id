# Red-team sandbox

A page that hands visitors a real agent key and lets them attack it: one-click attacks, their own payments, and an
email that Claude (with the agent key behind its tools) reads and acts on. The Leash ID contract refuses everything
outside the leash. Everything is answered by simulating against the live contract (same code path, no fee), except
one fixed control payment to our own vendor address, which is broadcast under a daily cap.

Status: the sandbox's leash is live on ME Hub testnet: agent `me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan`, owner ME ID
2611255912050, ≤ 0.02 MEC per payment, ≤ 0.05 MEC per 24 h, our vendor only. The agent key holds **0 MEC**: the owner
pays its fees through a fee grant (contract calls only). Every preset was tried against it from a local server; hosting
needs only the Vercel step below.

## Deploy on Vercel

1. Import the GitHub repo into Vercel (framework preset: Other, root directory: repo root). `vercel.json` serves
   `sandbox/index.html` at `/` and the functions in `api/`.
2. Environment variables:
   - `LEASH_AGENT_MNEMONIC`: the **sandbox agent's** key only. Never an owner key.
   - `LEASH_FEE_GRANTER`: the owner's address, so fees come from the owner's fee grant (the agent key holds no MEC).
   - `ANTHROPIC_API_KEY`: for the "prompt-inject Claude" box.
   - optional: `LEASH_CONTRACT`, `ME_RPC`, `ME_REST`, `ME_CHAIN_ID` (default: testnet and `deployment.json`),
     `SANDBOX_PER_IP` (40 per 10 min), `SANDBOX_PER_DAY` (2000 actions), `SANDBOX_MODEL` (`claude-opus-5-5`).
3. On chain, before opening it up (owner side, from a machine that holds the owner key):
   ```bash
   leash keygen sandbox-agent                 # then put that mnemonic in LEASH_AGENT_MNEMONIC
   leash deposit 0.2
   leash offer <sandbox agent address> --per-tx 0.02 --daily 0.05 --approve-above 0.015 --vendors <your vendor address> --any-verified-vendor no
   leash gas-grant <sandbox agent address> --limit 0.3 --days 40   # the owner pays its fees; the key holds 0 MEC
   leash accept <owner address> --as sandbox-agent --fee-granter <owner address>
   ```
   Keep the budget small: the sandbox is meant to be attacked.

## Run locally

```bash
source devnet/env.sh            # or point ME_* at testnet
node scripts/sandbox-dev.mjs    # http://localhost:18780/
```
