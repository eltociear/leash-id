// Sending as a role and telling apart "the chain refused" from "something broke on our side".
//
// By default every action is broadcast with a fixed gas limit instead of being simulated first, so a
// refusal is a real failed transaction on ME Hub with a hash anyone can look up, not just an error in
// our terminal. LEASH_SIMULATE=1 switches back to simulate-first (refusals then cost nothing and
// leave no trace on chain).
import { NET, contract, signer, txUrl } from "./chain.mjs";

const GAS = Number(process.env.LEASH_GAS ?? 600_000);
const price = Number(NET.gasPrice.replace(/[a-z]+$/, ""));
const fixedFee = () => ({ amount: [{ denom: NET.denom, amount: String(Math.ceil(GAS * price)) }], gas: String(GAS) });
const feeFor = () => (process.env.LEASH_SIMULATE === "1" ? "auto" : fixedFee());

/** "…message index: 0: over daily limit: 3 spent + 1 > 3: execute wasm contract failed" -> "over daily limit: 3 spent + 1 > 3" */
export function contractReason(raw) {
  const s = String(raw ?? "");
  const m = s.match(/message index: \d+: (.*?): execute wasm contract failed/s);
  if (m) return m[1].trim();
  if (/insufficient funds|insufficient fee/i.test(s)) return s.match(/[^:]*insufficient (funds|fee)[^;]*/i)?.[0].trim() ?? s;
  if (/unauthorized|out of gas|account sequence mismatch/i.test(s)) return s;
  return null;
}

function classify(e) {
  const msg = String(e?.message ?? e);
  const hash = msg.match(/broadcasting tx ([0-9A-F]{64})/)?.[1];
  const reason = contractReason(msg);
  if (reason && !/out of gas|account sequence mismatch/i.test(reason)) {
    return { ok: false, refused: true, reason, hash, link: hash ? txUrl(hash) : undefined };
  }
  return { ok: false, infra: true, reason: msg.slice(0, 400), hash, link: hash ? txUrl(hash) : undefined };
}

function ok(r) {
  const attrs = Object.fromEntries(
    (r.events ?? []).filter((e) => e.type === "wasm").flatMap((e) => e.attributes)
      .filter((a) => a.key !== "_contract_address").map((a) => [a.key, a.value]),
  );
  return { ok: true, hash: r.transactionHash, link: txUrl(r.transactionHash), gasUsed: Number(r.gasUsed), attrs };
}

/** Execute a Leash ID message as `role`. Never throws. */
export async function act(role, msg, funds = []) {
  try {
    const { client, addr } = await signer(role);
    return ok(await client.execute(addr, contract(), msg, feeFor(), "leash-id", funds));
  } catch (e) {
    return classify(e);
  }
}

/** A plain bank send as `role` (used to show what the agent key can and cannot move by itself). */
export async function send(role, to, umec) {
  try {
    const { client, addr } = await signer(role);
    return ok(await client.sendTokens(addr, to, [{ denom: NET.denom, amount: String(umec) }], feeFor(), "leash-id"));
  } catch (e) {
    return classify(e);
  }
}
