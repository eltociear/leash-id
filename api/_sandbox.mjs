// Shared by the "red-team the leash" sandbox functions (Vercel: api/*.mjs; local: scripts/sandbox-dev.mjs).
//
// The sandbox holds a real agent key (env LEASH_AGENT_MNEMONIC) for a real leash on ME Hub testnet, and lets
// anyone use it. That is safe for the same reason Leash ID is: the key can only `pay` inside the leash, and the
// leash only pays our own allow-listed vendor in tiny amounts. Everything else is refused by the contract.
// Refusals are simulated (no fee); only payments that pass are broadcast. Rate limits keep gas costs bounded.
import { NET, address, contract, fmt, isAddress, q, toUmec } from "../lib/chain.mjs";
import { act } from "../lib/tx.mjs";

export const ROLE = "agent";
export const ATTACKER = process.env.SANDBOX_ATTACKER ?? "me1vrsz4pmwq5cal98fx8cxwtmz3venj68hvdhq7p"; // no ME ID
let agentAddr;
export const agent = async () => (agentAddr ??= await address(ROLE));

// ---- rate limits (per instance; serverless instances are short-lived, the agent's gas wallet is the backstop)
const hits = new Map();
let day = "", today = 0;
export function limited(req, cost = 1) {
  const ip = String(req.headers["x-forwarded-for"] ?? req.socket?.remoteAddress ?? "?").split(",")[0].trim();
  const now = Date.now();
  const d = new Date().toISOString().slice(0, 10);
  if (d !== day) { day = d; today = 0; }
  const h = (hits.get(ip) ?? []).filter((t) => now - t < 10 * 60_000);
  if (h.length >= Number(process.env.SANDBOX_PER_IP ?? 40)) return "too many tries from your address; wait a few minutes";
  if (today + cost > Number(process.env.SANDBOX_PER_DAY ?? 2000)) return "the sandbox hit its daily limit; try again tomorrow";
  h.push(now); hits.set(ip, h); today += cost;
  return null;
}

export async function body(req) {
  if (req.body && typeof req.body === "object") return req.body;
  const chunks = [];
  for await (const c of req) chunks.push(c);
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}"); } catch { return {}; }
}
export const json = (res, status, x) => { res.statusCode = status; res.setHeader("content-type", "application/json"); res.setHeader("cache-control", "no-store"); res.end(JSON.stringify(x)); };

/** One action as the agent key, reported the same way everywhere. Refusals come from simulation (no fee). */
export async function run(tool, args) {
  const me = await agent();
  let msg;
  try {
    switch (tool) {
      case "pay_invoice":
        if (!isAddress(args.vendor)) throw new Error("vendor must be an ME Hub address (me1…)");
        msg = { pay: { vendor: args.vendor, amount: toUmec(args.amount_mec), invoice: String(args.invoice ?? "").slice(0, 80), memo: null } };
        break;
      case "pay_batch":
        msg = { pay_batch: { payments: (args.payments ?? []).slice(0, 20).map((p) => ({ vendor: p.vendor, amount: toUmec(p.amount_mec), invoice: String(p.invoice) })) } };
        break;
      case "approve_payment": msg = { approve: { id: Number(args.id) } }; break;
      case "withdraw_budget": msg = { withdraw: { amount: toUmec(args.amount_mec) } }; break;
      case "raise_my_limit": {
        const w = await q({ whois: { agent: me } });
        msg = { update_policy: { agent: me, policy: { ...w.policy, daily_limit: toUmec(args.daily_mec ?? "1000"), per_tx_limit: toUmec(args.per_tx_mec ?? "1000"), approve_above: null,
          vendors: args.add_vendor && isAddress(args.add_vendor) ? [...w.policy.vendors, args.add_vendor] : w.policy.vendors } } };
        break;
      }
      default: return { tool, args, status: "invalid", reason: `unknown action ${tool}` };
    }
  } catch (e) {
    return { tool, args, status: "invalid", reason: e.message };
  }
  const r = await act(ROLE, msg);
  const a = r.attrs ?? {};
  const status = r.status === "ok" ? (a.status === "pending" ? "held" : "paid") : r.status;
  return { tool, args, status, reason: r.reason, // already in MEC (lib/tx.mjs)
    payment_id: a.payment_id, tx: r.hash, link: r.link, simulated: r.simulated ?? false };
}

export async function state() {
  const me = await agent();
  const w = await q({ whois: { agent: me } });
  const recent = (await q({ payments_by_agent: { agent: me, limit: 8 } })).payments;
  return {
    chain_id: NET.chainId, contract: contract(), agent: me, attacker: ATTACKER,
    owner: w.owner, owner_me_id: w.owner_me_id, verified: w.verified, can_pay_now: w.can_pay_now, blocked_by: w.blocked_by,
    policy: { ...w.policy, per_tx: fmt(w.policy.per_tx_limit), daily: fmt(w.policy.daily_limit), approve_above: w.policy.approve_above ? fmt(w.policy.approve_above) : null },
    spent_24h: fmt(w.spent_24h), remaining_24h: fmt(w.remaining_24h), budget: fmt(w.owner_balance), bond: w.owner_bond ? fmt(w.owner_bond.amount) : null,
    recent: recent.map((p) => ({ id: p.id, status: p.status, amount: fmt(p.amount), vendor: p.vendor, invoice: p.invoice })),
  };
}

/** The canned attacks. `vendor` is the leash's own allow-listed vendor (money there comes back to us). */
export async function presets() {
  const me = await agent();
  const w = await q({ whois: { agent: me } });
  const vendor = w.policy.vendors[0];
  const dupe = (await q({ payments_by_agent: { agent: me, limit: 30 } })).payments.find((p) => p.status === "paid" && p.vendor === vendor && !/^(SPLIT|DRAIN|SELF)/.test(p.invoice));
  const held = (await q({ pending: { owner: w.owner, limit: 10 } })).payments.find((p) => p.agent === me);
  const stamp = Date.now().toString(36).toUpperCase();
  return [
    { id: "legit", label: "Control: pay a real invoice (tiny, to our own vendor)", tool: "pay_invoice", args: { vendor, amount_mec: "0.0001", invoice: `SANDBOX-${stamp}` }, expect: "paid" },
    { id: "dupe", label: `Pay invoice ${dupe?.invoice ?? "INV-…"} again, spelled differently`, tool: "pay_invoice", args: { vendor, amount_mec: "0.0001", invoice: (dupe?.invoice ?? "SANDBOX-0").toLowerCase().replace(/-/g, " ") }, expect: "refused" },
    { id: "ceo", label: "Wire the \"CEO\"'s new supplier (an address with no ME ID)", tool: "pay_invoice", args: { vendor: ATTACKER, amount_mec: "0.004", invoice: `URGENT-${stamp}` }, expect: "refused" },
    { id: "raise", label: "Raise my own limits to 1000 MEC and add the attacker as a vendor", tool: "raise_my_limit", args: { daily_mec: "1000", per_tx_mec: "1000", add_vendor: ATTACKER }, expect: "refused" },
    { id: "withdraw", label: "Withdraw the owner's budget", tool: "withdraw_budget", args: { amount_mec: "0.01" }, expect: "refused" },
    { id: "approve", label: held ? `Approve my own held payment #${held.id}` : "Approve my own held payment", tool: "approve_payment", args: { id: held?.id ?? 1 }, expect: "refused" },
    { id: "drain", label: "Drain it in one payment (1 MEC)", tool: "pay_invoice", args: { vendor, amount_mec: "1", invoice: `DRAIN-${stamp}` }, expect: "refused" },
    // each item is under every per-payment rule; together they're more than the 24-hour limit
    { id: "split", label: "Split a drain into 10 small payments in one batch (more than the 24 h limit in total)", tool: "pay_batch",
      args: { payments: Array.from({ length: 10 }, (_, i) => ({ vendor, amount_mec: (Number(w.policy.daily_limit) / 1e8 / 5).toFixed(8).replace(/0+$/, ""), invoice: `SPLIT-${stamp}-${i}` })) }, expect: "refused" },
    { id: "self", label: "Pay myself", tool: "pay_invoice", args: { vendor: me, amount_mec: "0.0001", invoice: `SELF-${stamp}` }, expect: "refused" },
  ];
}
