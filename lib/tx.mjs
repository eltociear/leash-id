// Sending transactions so that the caller always knows what happened.
//
// Every send ends in exactly one outcome:
//   ok        included, code 0                       (attrs/events from the chain)
//   refused   the Leash ID contract said no          (no fee if caught in simulation; hash if broadcast)
//   failed    included but failed (e.g. out of gas)  (fee charged, hash)
//   unknown   broadcast, but inclusion not confirmed (hash; MAY HAVE GONE THROUGH: look it up before retrying)
//   not_sent  nothing reached the chain             (safe to retry)
//
// The hash is computed from the signed bytes before broadcasting, so "unknown" always carries a hash
// that can be looked up later. Sends from one key go through a queue, so parallel calls can't collide
// on the account sequence.
import { sha256 } from "@cosmjs/crypto";
import { toHex, toUtf8 } from "@cosmjs/encoding";
import { MsgExecuteContract } from "cosmjs-types/cosmwasm/wasm/v1/tx";
import { MsgSend } from "cosmjs-types/cosmos/bank/v1beta1/tx";
import { TxRaw } from "cosmjs-types/cosmos/tx/v1beta1/tx";
import { Registry } from "@cosmjs/proto-signing";
import { defaultRegistryTypes } from "@cosmjs/stargate";
import { wasmTypes } from "@cosmjs/cosmwasm-stargate";
import { NET, contract, fmt, reader, retry, signer, txUrl } from "./chain.mjs";

const GAS_ADJ = Number(process.env.LEASH_GAS_ADJUSTMENT ?? 1.4);
const FIXED_GAS = Number(process.env.LEASH_GAS ?? 600_000);
// read when used, so a host (e.g. the sandbox, inside a 60 s function limit) can shorten it after import
const waitMs = () => Number(process.env.LEASH_TX_TIMEOUT_MS ?? 60_000);
const price = Number(NET.gasPrice.replace(/[a-z]+$/i, ""));
// `granter`: an x/feegrant allowance pays the fee (the owner's, so the agent key can hold no MEC at all).
const feeFor = (gas, granter) => ({ amount: [{ denom: NET.denom, amount: String(Math.ceil(gas * price)) }], gas: String(gas), ...(granter ? { granter } : {}) });
/** Demo mode: broadcast even what simulation refuses, so every refusal is a failed tx anyone can look up. */
const onchainRefusals = () => process.env.LEASH_ONCHAIN_REFUSALS === "1";

// The contract reports amounts in umec; show them in MEC: "over daily limit: 4900000 spent in the last
// 24h + 1400000 > 5000000" -> "over daily limit: 0.049 MEC spent in the last 24h + 0.014 MEC > 0.05 MEC".
export const mecAmounts = (r) => r.replace(/^((?:over per-payment|over daily) limit|insufficient owner budget): (.*)$/,
  (_, k, rest) => `${k}: ${rest.replace(/\b\d+\b(?! ?h\b)/g, (n) => fmt(n))}`);

/** The contract's own refusal reason, or null when the error isn't a contract decision. */
export function contractReason(raw) {
  const s = String(raw ?? "");
  const m = s.match(/(?:message index: \d+: )?(.*?): execute wasm contract failed/s);
  if (!m) return null;
  return mecAmounts(m[1].replace(/^.*?failed to execute message; message index: \d+: /s, "").trim());
}

/** Errors from the sender's own wallet (not the contract): no gas money, bad fee, sequence. */
function walletProblem(s) {
  if (/fee-grant not found|fee allowance|basic allowance|message does not exist in allowed messages/i.test(s)) return "the fee grant doesn't cover this (expired, used up, or not for this kind of message)";
  if (/insufficient fee/i.test(s)) return "the fee is too low or nobody pays it (top the key up with a little MEC, or have the owner grant fees: leash gas-grant)";
  if (/insufficient funds/i.test(s)) return "the sending key doesn't hold enough MEC for this (the amount, or the fee if no grant covers it)";
  if (/account sequence mismatch|incorrect account sequence/i.test(s)) return "account sequence mismatch (another send from this key raced it)";
  if (/account .* not found|does not exist on chain/i.test(s)) return "the sending key has never received MEC, so it has no account on chain yet";
  return null;
}

function attrsOf(events = []) {
  const out = {};
  for (const e of events) {
    if (e.type !== "wasm") continue;
    for (const a of e.attributes) if (a.key !== "_contract_address") out[a.key] = a.value;
  }
  return out;
}

const queues = new Map();
function serial(key, fn) {
  const prev = queues.get(key) ?? Promise.resolve();
  const run = prev.then(fn, fn);
  queues.set(key, run.catch(() => {}));
  return run;
}

async function waitFor(client, hash) {
  const until = Date.now() + waitMs();
  while (Date.now() < until) {
    try {
      const tx = await client.getTx(hash);
      if (tx) return tx;
    } catch {}
    await new Promise((r) => setTimeout(r, 1500));
  }
  return null;
}

// 0 turns the check off (e.g. a devnet that only makes blocks on demand)
const maxBlockAge = () => Number(process.env.LEASH_MAX_BLOCK_AGE_S ?? 180);
/** {height, ago} when the latest block is too old to broadcast into, else null. A failed check never blocks. */
export async function stall(client) {
  if (!maxBlockAge() || typeof client?.getBlock !== "function") return null;
  try {
    const { header } = await client.getBlock();
    const age = (Date.now() - Date.parse(header.time)) / 1000;
    if (!(age > maxBlockAge())) return null;
    return { height: header.height, ago: age >= 5400 ? `${(age / 3600).toFixed(1)} h` : `${Math.round(age / 60)} min` };
  } catch {
    return null;
  }
}
const halted = (st) => `ME Hub isn't producing blocks (the latest, #${st.height}, is ${st.ago} old)`;
/** Why not to broadcast now, or null. */
export async function stalledFor(client) {
  const st = await stall(client);
  return st && `${halted(st)}, so nothing was sent; it's safe to retry once the chain moves`;
}
/** For read-only answers ("can pay: yes"): a note that nothing can actually be paid until the chain moves, or null. */
export async function chainNote() {
  const st = await reader().then(stall).catch(() => null);
  return st && `${halted(st)}: no payment can land until it moves`;
}

/** Sign and send `msgs` as `role` (or the key in `file`). Never throws. */
/** `dryRun: true` stops after simulation: "simulated_ok" if the chain would accept it, nothing broadcast. */
export async function sendTx(role, msgs, { memo = "leash-id", file, dryRun = false, granter = process.env.LEASH_FEE_GRANTER || undefined } = {}) {
  let s;
  try {
    s = await retry(() => signer(role, file)); // connecting is safe to repeat
  } catch (e) {
    return { status: "not_sent", reason: String(e?.message ?? e).slice(0, 300) };
  }
  return serial(s.addr, () => sendNow(s, msgs, memo, dryRun, granter));
}

/** Exported for tests: the whole send path against any client with cosmjs's signing-client shape. */
export async function sendNow({ client, addr }, msgs, memo, dryRun, granter) {
  // 1. Simulate: a refusal here costs nothing and leaves nothing on chain.
  let gas;
  try {
    // simulation changes nothing, so a dropped connection is simply tried again
    gas = Math.ceil((await retry(() => client.simulate(addr, msgs, memo))) * GAS_ADJ);
  } catch (e) {
    const s = String(e?.message ?? e);
    const reason = contractReason(s);
    if (reason && onchainRefusals() && !dryRun) gas = FIXED_GAS; // demo: record the refusal on chain as well
    else if (reason) return { status: "refused", reason, simulated: true };
    else return { status: "not_sent", reason: walletProblem(s) ?? s.slice(0, 300) };
  }
  if (dryRun) return { status: "simulated_ok", gas };
  // 1b. Is the chain making blocks? Testnets halt now and then. A tx broadcast into a halted chain sits in the
  // mempool and comes back "unknown"; it may then land hours later. Better to say so now and send nothing.
  const stalled = await stalledFor(client);
  if (stalled) return { status: "not_sent", reason: stalled };
  // 2. Sign; the hash is known before anything leaves this process.
  let bytes, hash;
  try {
    const raw = await retry(() => client.sign(addr, msgs, feeFor(gas, granter), memo)); // reads the account; sends nothing
    bytes = TxRaw.encode(raw).finish();
    hash = toHex(sha256(bytes)).toUpperCase();
  } catch (e) {
    const s = String(e?.message ?? e);
    return { status: "not_sent", reason: walletProblem(s) ?? s.slice(0, 300) };
  }
  // 3. Broadcast. CheckTx rejections mean "not in the mempool": nothing was sent.
  try {
    await client.broadcastTxSync(bytes);
  } catch (e) {
    const s = String(e?.message ?? e);
    if (e?.code !== undefined && e?.codespace !== undefined) {
      // sdk code 19 "tx already in mempool": these very bytes did reach a node, so they may still land
      if (e.codespace === "sdk" && Number(e.code) === 19) return { status: "unknown", hash, link: txUrl(hash), reason: "already in the mempool" };
      const reason = contractReason(s);
      if (reason) return { status: "refused", reason, hash: undefined };
      return { status: "not_sent", reason: walletProblem(s) ?? s.slice(0, 300) };
    }
    // Transport error after the bytes left: the tx may still land.
    return { status: "unknown", hash, link: txUrl(hash), reason: `broadcast interrupted: ${s.slice(0, 200)}` };
  }
  // 4. Wait for inclusion and report what the chain actually did.
  const tx = await waitFor(client, hash);
  if (!tx) return { status: "unknown", hash, link: txUrl(hash), reason: `not seen on chain within ${waitMs() / 1000}s` };
  const base = { hash, link: txUrl(hash), gasUsed: Number(tx.gasUsed), height: tx.height };
  if (tx.code === 0) return { status: "ok", ...base, attrs: attrsOf(tx.events), events: tx.events };
  const log = tx.rawLog ?? "";
  const reason = contractReason(log);
  if (reason) return { status: "refused", ...base, reason };
  if (/out of gas/i.test(log)) return { status: "failed", ...base, reason: "out of gas (the fee was charged; nothing else happened)" };
  return { status: "failed", ...base, reason: log.slice(0, 300) };
}

export const executeMsg = (sender, msg, funds = []) => ({
  typeUrl: "/cosmwasm.wasm.v1.MsgExecuteContract",
  value: MsgExecuteContract.fromPartial({ sender, contract: contract(), msg: toUtf8(JSON.stringify(msg)), funds }),
});

/** Execute a Leash ID message as `role`. Never throws. */
export async function act(role, msg, funds = [], opts = {}) {
  let addr;
  try { ({ addr } = await retry(() => signer(role, opts.file))); } catch (e) { return { status: "not_sent", reason: String(e?.message ?? e).slice(0, 300) }; }
  return sendTx(role, [executeMsg(addr, msg, funds)], opts);
}

/**
 * Dry-run a Leash ID message as any address, with no key: the chain's own simulation, using the public key the
 * account already published on chain (any account that has sent a tx). Same outcomes as `act(..., {dryRun: true})`:
 * "simulated_ok", "refused" with the contract's reason, or "not_sent". Never throws.
 */
export async function simulateAs(addr, msg, funds = []) {
  const keyless = {
    async simulate(signerAddress, msgs, memo) {
      const client = await reader();
      const acct = await client.getAccount(signerAddress);
      if (!acct?.pubkey) throw new Error(`${signerAddress} has no public key on chain yet (it has never sent a tx), so it can't be simulated without its key`);
      const anyMsgs = msgs.map((m) => registry.encodeAsAny(m));
      const sim = (seq) => client.forceGetQueryClient().tx.simulate(anyMsgs, memo, acct.pubkey, seq);
      let res;
      try {
        res = await sim(acct.sequence);
      } catch (e) {
        // A tx from this account still waiting in the mempool moves the sequence the node checks against.
        // Simulation verifies no signature, so ask again with the sequence the node expects.
        const m = String(e?.message ?? e).match(/account sequence mismatch, expected (\d+), got \d+/);
        if (!m) throw e;
        res = await sim(Number(m[1]));
      }
      return Number(res.gasInfo.gasUsed);
    },
  };
  try {
    return await sendNow({ client: keyless, addr }, [executeMsg(addr, msg, funds)], "leash-id", true);
  } catch (e) {
    return { status: "not_sent", reason: String(e?.message ?? e).slice(0, 300) };
  }
}
const registry = new Registry([...defaultRegistryTypes, ...wasmTypes]);

/** A plain bank send (used to show what the agent key can and cannot move by itself). */
export async function send(role, to, umec, opts = {}) {
  let addr;
  try { ({ addr } = await retry(() => signer(role, opts.file))); } catch (e) { return { status: "not_sent", reason: String(e?.message ?? e).slice(0, 300) }; }
  return sendTx(role, [{
    typeUrl: "/cosmos.bank.v1beta1.MsgSend",
    value: MsgSend.fromPartial({ fromAddress: addr, toAddress: to, amount: [{ denom: NET.denom, amount: String(umec) }] }),
  }], opts);
}

// ---------------- fee grants (x/feegrant) ----------------
import { MsgGrantAllowance, MsgRevokeAllowance } from "cosmjs-types/cosmos/feegrant/v1beta1/tx";
import { AllowedMsgAllowance, BasicAllowance } from "cosmjs-types/cosmos/feegrant/v1beta1/feegrant";

/** Owner -> agent: pay the agent's fees for contract executions only, up to `limitUmec`, until `expiresAt` (unix s). */
export function grantGasMsg(granter, grantee, limitUmec, expiresAt) {
  const basic = BasicAllowance.encode(BasicAllowance.fromPartial({
    spendLimit: [{ denom: NET.denom, amount: String(limitUmec) }],
    expiration: { seconds: BigInt(expiresAt), nanos: 0 },
  })).finish();
  const allowed = AllowedMsgAllowance.encode(AllowedMsgAllowance.fromPartial({
    allowance: { typeUrl: "/cosmos.feegrant.v1beta1.BasicAllowance", value: basic },
    allowedMessages: ["/cosmwasm.wasm.v1.MsgExecuteContract"],
  })).finish();
  return {
    typeUrl: "/cosmos.feegrant.v1beta1.MsgGrantAllowance",
    value: MsgGrantAllowance.fromPartial({ granter, grantee, allowance: { typeUrl: "/cosmos.feegrant.v1beta1.AllowedMsgAllowance", value: allowed } }),
  };
}
export const revokeGasMsg = (granter, grantee) => ({ typeUrl: "/cosmos.feegrant.v1beta1.MsgRevokeAllowance", value: MsgRevokeAllowance.fromPartial({ granter, grantee }) });
