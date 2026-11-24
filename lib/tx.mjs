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
import { NET, contract, fmt, signer, txUrl } from "./chain.mjs";

const GAS_ADJ = Number(process.env.LEASH_GAS_ADJUSTMENT ?? 1.4);
const FIXED_GAS = Number(process.env.LEASH_GAS ?? 600_000);
const WAIT_MS = Number(process.env.LEASH_TX_TIMEOUT_MS ?? 60_000);
const price = Number(NET.gasPrice.replace(/[a-z]+$/i, ""));
const feeFor = (gas) => ({ amount: [{ denom: NET.denom, amount: String(Math.ceil(gas * price)) }], gas: String(gas) });
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
  if (/insufficient funds|insufficient fee/i.test(s)) return "the sending key's own wallet can't pay the fee (top it up with a little MEC for gas)";
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
  const until = Date.now() + WAIT_MS;
  while (Date.now() < until) {
    try {
      const tx = await client.getTx(hash);
      if (tx) return tx;
    } catch {}
    await new Promise((r) => setTimeout(r, 1500));
  }
  return null;
}

/** Sign and send `msgs` as `role` (or the key in `file`). Never throws. */
/** `dryRun: true` stops after simulation: "simulated_ok" if the chain would accept it, nothing broadcast. */
export async function sendTx(role, msgs, { memo = "leash-id", file, dryRun = false } = {}) {
  let s;
  try {
    s = await signer(role, file);
  } catch (e) {
    return { status: "not_sent", reason: String(e?.message ?? e).slice(0, 300) };
  }
  return serial(s.addr, () => sendNow(s, msgs, memo, dryRun));
}

async function sendNow({ client, addr }, msgs, memo, dryRun) {
  // 1. Simulate: a refusal here costs nothing and leaves nothing on chain.
  let gas;
  try {
    gas = Math.ceil((await client.simulate(addr, msgs, memo)) * GAS_ADJ);
  } catch (e) {
    const s = String(e?.message ?? e);
    const reason = contractReason(s);
    if (reason && onchainRefusals() && !dryRun) gas = FIXED_GAS; // demo: record the refusal on chain as well
    else if (reason) return { status: "refused", reason, simulated: true };
    else return { status: "not_sent", reason: walletProblem(s) ?? s.slice(0, 300) };
  }
  if (dryRun) return { status: "simulated_ok", gas };
  // 2. Sign; the hash is known before anything leaves this process.
  let bytes, hash;
  try {
    const raw = await client.sign(addr, msgs, feeFor(gas), memo);
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
      const reason = contractReason(s);
      if (reason) return { status: "refused", reason, hash: undefined };
      return { status: "not_sent", reason: walletProblem(s) ?? s.slice(0, 300) };
    }
    // Transport error after the bytes left: the tx may still land.
    return { status: "unknown", hash, link: txUrl(hash), reason: `broadcast interrupted: ${s.slice(0, 200)}` };
  }
  // 4. Wait for inclusion and report what the chain actually did.
  const tx = await waitFor(client, hash);
  if (!tx) return { status: "unknown", hash, link: txUrl(hash), reason: `not seen on chain within ${WAIT_MS / 1000}s` };
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
  const { addr } = await signer(role, opts.file).catch(() => ({}));
  if (!addr) return sendTx(role, [], opts); // reports the key problem as not_sent
  return sendTx(role, [executeMsg(addr, msg, funds)], opts);
}

/** A plain bank send (used to show what the agent key can and cannot move by itself). */
export async function send(role, to, umec, opts = {}) {
  const { addr } = await signer(role, opts.file).catch(() => ({}));
  if (!addr) return sendTx(role, [], opts);
  return sendTx(role, [{
    typeUrl: "/cosmos.bank.v1beta1.MsgSend",
    value: MsgSend.fromPartial({ fromAddress: addr, toAddress: to, amount: [{ denom: NET.denom, amount: String(umec) }] }),
  }], opts);
}
