# Running Leash ID safely

The contract enforces the leash. Everything here is about not handing an agent a way around it.

## 1. The agent must never be able to read the owner key

An LLM agent with file or shell tools (Claude Code, most agent frameworks) can read any file its process can
read. If that includes the owner key, a prompt injection can tell it to read the key and sign `withdraw` or
`update_policy` as the owner. The contract can't tell the difference.

So the MCP server refuses to start if any other Leash ID key is readable from its process:

```
leash-id MCP: other Leash ID keys are readable from this process:
  /home/ap/.config/leash-id/keys/owner.mnemonic
```

Run the agent side as its own OS user, or in a container that only has the agent key:

```bash
# owner machine (or a hardware wallet): create the agent key, offer the leash
leash keygen agent                                   # ~/.config/leash-id/keys/agent.mnemonic
leash offer agent --per-tx 0.02 --daily 0.05 --approve-above 0.015 --vendors me1acme…
# give the agent key alone to the agent host, e.g. as a Docker secret, then on that host:
leash accept me1owner… --key-file /run/secrets/agent.mnemonic
```

```json
{ "mcpServers": { "leash-id": {
    "command": "leash-mcp",
    "env": { "LEASH_AGENT_KEY_FILE": "/run/secrets/agent.mnemonic" } } } }
```

`LEASH_DEMO_SHARED_KEYS=1` disables the check. It exists for the demo in this repo, where every role lives on
one laptop. Don't use it anywhere real money is involved.

## 2. Keys on disk

- One file per role, `~/.config/leash-id/keys/<role>.mnemonic`, mode 0600, directory 0700. `leash keygen`
  enforces both; the tools warn if a key file is readable by others.
- Pre-0.2 installs kept every role in one `keys.env`. Run `leash keygen --split`, then delete `keys.env`.
- Error messages never include key material.

## 3. The contract you point at

- `npm run verify` (or `LEASH_CONTRACT=me1… npm run verify`) checks that the deployed code is the reproducible
  build of this repo **and that it has no admin**. With an admin, anyone holding that key could migrate the
  contract to code that pays out the escrow.
- `leash deploy` deploys without an admin unless you pass `--admin`.

## 4. Gas: the one thing outside the leash

Whatever the agent key itself holds, it can move: the contract only guards the escrowed budget. So the agent key
should hold nothing. With `leash gas-grant <agent> --limit 0.01`, the owner pays the agent's fees through an
x/feegrant allowance that covers **contract calls only** (`MsgExecuteContract`), capped and with an expiry. The
agent key can then hold 0 MEC: it can still accept its leash and pay through it, and it has nothing else to send
(recorded on a real node: `runs/devnet-killswitch-*.txt`, step 8). The MCP server finds and uses the owner's grant
on its own. The cap bounds what a stolen key can burn in fees, and `leash gas-revoke` ends it.

Without a grant, keep the agent's own balance small. Every send is simulated first, so refusals cost nothing.

## 5. When a send's outcome is unknown

`UNKNOWN` means the transaction was broadcast but not confirmed in time. It may have gone through. Look it up
(`leash invoice <vendor> <invoice>`, the MCP tool `find_invoice`, or the tx hash) before paying again, and
never re-send the same bill under a different invoice number.

## Reporting a vulnerability

Open a GitHub security advisory on this repository (Security → Report a vulnerability), not a public issue.
