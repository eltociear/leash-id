// The vendor side of Leash ID: accept payments from AI agents only when a verified human answers for them.
//
//   import { paywall } from "leash-id/vendor";
//   http.createServer(paywall({ payTo: "me1…", priceUmec: "100000", minKyc: 2 }, (req, res, paid) => {
//     res.end(JSON.stringify({ data: …, paid_by: paid.owner_did }));
//   }));
//
// The flow is HTTP 402 Payment Required:
//   1. GET /x                         -> 402 { pay_to, amount, invoice, contract, chain }   (fresh invoice per request)
//   2. the agent pays through its leash (Leash ID `pay`, invoice = the one above)
//   3. GET /x  X-Leash-Payment: <tx>  -> the paywall reads that tx from ME Hub: a Leash ID payment to pay_to, for
//      that invoice and at least that amount, from an agent whose owner holds a live ME ID (KYC ≥ minKyc,
//      bond ≥ minBondUmec if asked). Each tx unlocks one response.
import crypto from "node:crypto";
import { NET, contract, fmt, isNotFound, q, reader } from "./chain.mjs";

/** Who answers for `agent`, and does that meet the vendor's bar? Never throws. */
export async function verifyPayer(agent, { minKyc = 2, minBondUmec = 0n, requireCanPay = false } = {}) {
  let w;
  try {
    w = await q({ whois: { agent } });
  } catch (e) {
    return { ok: false, reason: isNotFound(e) ? "not a Leash ID agent: no verified human answers for it" : `couldn't check: ${e.message}` };
  }
  const id = w.owner_me_id;
  const base = { owner: w.owner, owner_did: id?.did ?? null, kyc_level: id?.kyc_level ?? 0, bond: w.owner_bond?.amount ?? "0", whois: w };
  if (!w.verified) return { ok: false, ...base, reason: `owner's ME ID is not verified (${w.blocked_by ?? "inactive or below the contract's KYC level"})` };
  if ((id?.kyc_level ?? 0) < minKyc) return { ok: false, ...base, reason: `owner's KYC level ${id?.kyc_level} is below ${minKyc}` };
  if (BigInt(w.owner_bond?.amount ?? 0) < BigInt(minBondUmec)) return { ok: false, ...base, reason: `owner's bond ${fmt(w.owner_bond?.amount ?? 0)} is below ${fmt(minBondUmec)}` };
  if (requireCanPay && !w.can_pay_now) return { ok: false, ...base, reason: `agent can't pay right now: ${w.blocked_by}` };
  return { ok: true, ...base };
}

/** The Leash ID payment(s) in tx `hash`, from its `wasm-leash_payment` events. */
export async function paymentsInTx(hash) {
  const tx = await (await reader()).getTx(hash.toUpperCase());
  if (!tx) return null;
  if (tx.code !== 0) return [];
  const C = contract();
  return tx.events.filter((e) => e.type === "wasm-leash_payment").map((e) => Object.fromEntries(e.attributes.map((a) => [a.key, a.value])))
    .filter((a) => a._contract_address === C);
}

/**
 * HTTP 402 paywall for Node's http (or Express: same signature). `handler(req, res, payment)` runs only for a
 * request that carries a fresh, valid payment.
 */
export function paywall({ payTo, priceUmec, minKyc = 2, minBondUmec = 0n, ttlMs = 10 * 60_000, describe = "API call" }, handler) {
  if (!payTo) throw new Error("paywall: payTo (your ME Hub address) is required");
  const issued = new Map(); // invoice -> expires
  const spent = new Set(); // tx hashes already used
  const send = (res, status, body) => {
    res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
    res.end(JSON.stringify(body, null, 2));
  };
  return async (req, res) => {
    const now = Date.now();
    for (const [k, exp] of issued) if (exp < now) issued.delete(k);
    const tx = req.headers["x-leash-payment"];
    if (!tx) {
      const invoice = `API-${crypto.randomBytes(6).toString("hex").toUpperCase()}`;
      issued.set(invoice, now + ttlMs);
      return send(res, 402, {
        error: "payment required",
        leash_id: {
          pay_to: payTo, amount: String(priceUmec), amount_mec: fmt(priceUmec), invoice, chain_id: NET.chainId, contract: contract(),
          for: describe, expires_in_s: Math.round(ttlMs / 1000),
          accepts: `Leash ID payments from agents whose owner holds a live ME ID at KYC level ≥ ${minKyc}` + (BigInt(minBondUmec) > 0n ? ` with a bond of at least ${fmt(minBondUmec)}` : ""),
          retry_with_header: "X-Leash-Payment: <tx hash>",
        },
      });
    }
    const hash = String(tx).trim().toUpperCase();
    if (!/^[0-9A-F]{64}$/.test(hash)) return send(res, 400, { error: "X-Leash-Payment must be a transaction hash" });
    if (spent.has(hash)) return send(res, 402, { error: "that payment was already used for a response" });
    let pays;
    try { pays = await paymentsInTx(hash); } catch (e) { return send(res, 503, { error: `couldn't read ME Hub: ${e.message}` }); }
    if (!pays) return send(res, 402, { error: "no such transaction on ME Hub (yet?)" });
    const p = pays.find((a) => a.vendor === payTo && a.status === "paid" && ["created", "approved"].includes(a.kind) && issued.has(a.invoice) && BigInt(a.amount) >= BigInt(priceUmec));
    if (!p) return send(res, 402, { error: `no Leash ID payment in that tx pays ${fmt(priceUmec)} to ${payTo} for an invoice this server issued` });
    const who = await verifyPayer(p.agent, { minKyc, minBondUmec });
    if (!who.ok) return send(res, 403, { error: `payment received, but ${who.reason}`, payment_id: p.payment_id });
    spent.add(hash);
    issued.delete(p.invoice);
    return handler(req, res, { tx: hash, payment_id: Number(p.payment_id), agent: p.agent, owner: p.owner, owner_did: p.owner_did, amount: p.amount, invoice: p.invoice });
  };
}
