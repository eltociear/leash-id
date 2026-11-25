// The vendor side of Leash ID: accept payments from AI agents only when a verified human answers for them.
//
//   import { paywall } from "leash-id/vendor";
//   http.createServer(paywall({ payTo: "me1…", priceUmec: "100000", minKyc: 2 }, (req, res, paid) => {
//     res.end(JSON.stringify({ data: …, paid_by: paid.owner_did }));
//   }));
//
// The flow is HTTP 402 Payment Required:
//   1. GET /x                        -> 402 { pay_to, amount, invoice, contract, chain }  (a fresh, signed invoice)
//   2. the agent pays through its leash (Leash ID `pay`, invoice = the one above)
//   3. GET /x  X-Leash-Payment: <tx>, X-Leash-Proof: <the agent key's signature over "<tx>:<invoice>">
//      -> the paywall reads the tx from ME Hub: a Leash ID payment to pay_to, for that invoice, at least that
//      amount, made by the agent that signed the proof, whose owner holds a live ME ID (KYC ≥ minKyc, bond ≥
//      minBondUmec locked for ≥ minBondLockS if asked). Each tx unlocks one response.
//
// Tx hashes are public, so the proof is what stops someone who watched the chain from redeeming another
// agent's payment. Invoices carry their own expiry and an HMAC, so the server keeps no per-request state.
// The one thing it remembers is which txs already bought a response (each tx unlocks one). By default that is
// in this process: right for one server. Behind several replicas, or across restarts with a fixed `secret`,
// pass `spent`: a shared store with async claim(hash, untilMs) -> true if newly claimed, and release(hash).
import crypto from "node:crypto";
import { NET, contract, fmt, isNotFound, q, reader, retry, verifyProof } from "./chain.mjs";

/** Who answers for `agent`, and does that meet the vendor's bar? Never throws. */
export async function verifyPayer(agent, { minKyc = 2, minBondUmec = 0n, minBondLockS = 0, requireCanPay = false } = {}) {
  let w;
  try {
    w = await q({ whois: { agent } });
  } catch (e) {
    return { ok: false, reason: isNotFound(e) ? "not a Leash ID agent: no verified human answers for it" : "couldn't check the agent on ME Hub" };
  }
  const id = w.owner_me_id;
  const bond = w.owner_bond;
  const base = { owner: w.owner, owner_did: id?.did ?? null, kyc_level: id?.kyc_level ?? 0, bond: bond?.amount ?? "0", whois: w };
  if (!w.verified) return { ok: false, ...base, reason: `owner's ME ID is not verified (${w.blocked_by ?? "inactive or below the contract's KYC level"})` };
  if ((id?.kyc_level ?? 0) < minKyc) return { ok: false, ...base, reason: `owner's KYC level ${id?.kyc_level} is below ${minKyc}` };
  if (BigInt(minBondUmec) > 0n) {
    if (BigInt(bond?.amount ?? 0) < BigInt(minBondUmec)) return { ok: false, ...base, reason: `owner's bond ${fmt(bond?.amount ?? 0)} is below ${fmt(minBondUmec)}` };
    if ((bond.earliest_unlock ?? 0) < Date.now() / 1000 + minBondLockS) return { ok: false, ...base, reason: "owner's bond unlocks too soon" };
  }
  if (requireCanPay && !w.can_pay_now) return { ok: false, ...base, reason: `agent can't pay right now: ${w.blocked_by}` };
  return { ok: true, ...base };
}

/** The Leash ID payment(s) in tx `hash`, from its `wasm-leash_payment` events (wasmd sets _contract_address). */
export async function paymentsInTx(hash) {
  const tx = await retry(async () => (await reader()).getTx(hash.toUpperCase())); // reads are safe to repeat
  if (!tx) return null;
  if (tx.code !== 0) return [];
  const C = contract();
  return tx.events.filter((e) => e.type === "wasm-leash_payment").map((e) => Object.fromEntries(e.attributes.map((a) => [a.key, a.value])))
    .filter((a) => a._contract_address === C);
}

/** The text an agent signs to redeem payment `tx` for `invoice`. */
export const proofText = (tx, invoice) => `leash-id paywall: ${String(tx).toUpperCase()}:${invoice}`;

/**
 * HTTP 402 paywall for Node's http (or Express: same signature). `handler(req, res, payment)` runs only for a
 * request that carries a fresh, valid payment and the payer's proof.
 */
export function paywall({ payTo, priceUmec, minKyc = 2, minBondUmec = 0n, minBondLockS = 0, ttlMs = 10 * 60_000, describe = "API call", secret, spent: store }, handler) {
  if (!payTo) throw new Error("paywall: payTo (your ME Hub address) is required");
  if (store && (typeof store.claim !== "function" || typeof store.release !== "function")) throw new Error("paywall: spent must have claim(hash, untilMs) and release(hash)");
  const key = secret ?? crypto.randomBytes(32);
  const mac = (s) => crypto.createHmac("sha256", key).update(s).digest("hex").slice(0, 16).toUpperCase();
  const issue = () => {
    const exp = Math.floor((Date.now() + ttlMs) / 1000).toString(36).toUpperCase();
    const nonce = crypto.randomBytes(5).toString("hex").toUpperCase();
    return `API-${exp}-${nonce}-${mac(`${exp}-${nonce}-${payTo}-${priceUmec}`)}`;
  };
  const valid = (inv) => {
    const m = /^API-([0-9A-Z]+)-([0-9A-F]{10})-([0-9A-F]{16})$/.exec(String(inv));
    if (!m) return false;
    const good = crypto.timingSafeEqual(Buffer.from(m[3]), Buffer.from(mac(`${m[1]}-${m[2]}-${payTo}-${priceUmec}`)));
    return good && parseInt(m[1], 36) * 1000 >= Date.now() - 60_000; // a minute's grace for the payment to land
  };
  const spent = new Map(); // tx -> forget after
  const send = (res, status, body) => {
    res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
    res.end(JSON.stringify(body, null, 2));
  };
  return async (req, res) => {
    const now = Date.now();
    for (const [k, until] of spent) if (until < now) spent.delete(k);
    const tx = req.headers["x-leash-payment"];
    if (!tx) {
      return send(res, 402, {
        error: "payment required",
        leash_id: {
          pay_to: payTo, amount: String(priceUmec), amount_mec: fmt(priceUmec), invoice: issue(), chain_id: NET.chainId, contract: contract(),
          for: describe, expires_in_s: Math.round(ttlMs / 1000),
          accepts: `Leash ID payments from agents whose owner holds a live ME ID at KYC level ≥ ${minKyc}` + (BigInt(minBondUmec) > 0n ? ` with a bond of at least ${fmt(minBondUmec)}` : ""),
          retry_with_headers: { "X-Leash-Payment": "<tx hash>", "X-Leash-Proof": "base64 JSON {pub_key, signature}: the agent key's ADR-36 signature over \"leash-id paywall: <TX>:<invoice>\"" },
        },
      });
    }
    const hash = String(tx).trim().toUpperCase();
    if (!/^[0-9A-F]{64}$/.test(hash)) return send(res, 400, { error: "X-Leash-Payment must be a transaction hash" });
    let proof;
    try { proof = JSON.parse(Buffer.from(String(req.headers["x-leash-proof"] ?? ""), "base64").toString("utf8")); } catch {}
    if (!proof?.pub_key || !proof?.signature) return send(res, 401, { error: "X-Leash-Proof is missing: the paying agent must sign the tx and invoice" });
    // Claim the tx before reading the chain, so two concurrent requests can't both redeem it.
    let release;
    if (store) {
      let fresh;
      try { fresh = await store.claim(hash, now + ttlMs + 120_000); } catch { return send(res, 503, { error: "couldn't record the payment; try again" }); }
      if (!fresh) return send(res, 402, { error: "that payment was already used for a response" });
      release = () => Promise.resolve(store.release(hash)).catch(() => {});
    } else {
      if (spent.has(hash)) return send(res, 402, { error: "that payment was already used for a response" });
      spent.set(hash, now + ttlMs + 120_000);
      release = () => spent.delete(hash);
    }
    let pays;
    try { pays = await paymentsInTx(hash); } catch { release(); return send(res, 503, { error: "couldn't read ME Hub; try again" }); }
    if (!pays) { release(); return send(res, 402, { error: "no such transaction on ME Hub (yet?)" }); }
    const p = pays.find((a) => a.vendor === payTo && a.status === "paid" && ["created", "approved"].includes(a.kind) && valid(a.invoice) && BigInt(a.amount) >= BigInt(priceUmec));
    if (!p) { release(); return send(res, 402, { error: `no Leash ID payment in that tx pays ${fmt(priceUmec)} to ${payTo} for a live invoice this server issued` }); }
    if (!(await verifyProof({ signer: p.agent, ...proof }, proofText(hash, p.invoice)))) {
      release();
      return send(res, 403, { error: "X-Leash-Proof isn't a signature by the agent that made this payment" });
    }
    const who = await verifyPayer(p.agent, { minKyc, minBondUmec, minBondLockS });
    if (!who.ok) { release(); return send(res, 403, { error: `payment received, but ${who.reason}`, payment_id: p.payment_id }); }
    try {
      return await handler(req, res, { tx: hash, payment_id: Number(p.payment_id), agent: p.agent, owner: p.owner, owner_did: p.owner_did, amount: p.amount, invoice: p.invoice });
    } catch {
      release(); // the buyer paid and got nothing: let the same proof retry
      if (!res.headersSent) send(res, 500, { error: "the API failed after payment; retry with the same headers" });
    }
  };
}
