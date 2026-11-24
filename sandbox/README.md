# Red-team sandbox

A page that hands visitors a real agent key and lets them attack it: one-click attacks, their own payments, and an
email that Claude (with the agent key behind its tools) reads and acts on. The Leash ID contract refuses everything
outside the leash. Everything is answered by simulating against the live contract (same code path, no fee), except
one fixed control payment to our own vendor address, which is broadcast under a daily cap.

Status: runs locally against a devnet (`node scripts/sandbox-dev.mjs`). Hosting it on testnet needs a leash, and a
leash needs an owner with a testnet ME ID; ours is pending.

## Deploy on Vercel

1. Import the GitHub repo into Vercel (framework preset: Other, root directory: repo root). `vercel.json` serves
   `sandbox/index.html` at `/` and the functions in `api/`.
2. Environment variables:
   - `LEASH_AGENT_MNEMONIC`: the **sandbox agent's** key only. Never an owner key.
   - `ANTHROPIC_API_KEY`: for the "prompt-inject Claude" box.
   - optional: `LEASH_CONTRACT`, `ME_RPC`, `ME_REST`, `ME_CHAIN_ID` (default: testnet and `deployment.json`),
     `SANDBOX_PER_IP` (40 per 10 min), `SANDBOX_PER_DAY` (2000 actions), `SANDBOX_MODEL` (`claude-opus-5-5`).
3. On chain, before opening it up (owner side, from a machine that holds the owner key):
   ```bash
   leash keygen sandbox-agent                 # then put that mnemonic in LEASH_AGENT_MNEMONIC
   leash deposit 0.2
   leash offer <sandbox agent address> --per-tx 0.02 --daily 0.05 --approve-above 0.015 --vendors <your vendor address> --any-verified-vendor no
   leash accept <owner address> --as sandbox-agent
   # send the agent ~0.05 MEC for gas; the leash budget stays with the contract
   ```
   Keep the budget small: the sandbox is meant to be attacked.

## Run locally

```bash
source devnet/env.sh            # or point ME_* at testnet
node scripts/sandbox-dev.mjs    # http://localhost:18780/
```
