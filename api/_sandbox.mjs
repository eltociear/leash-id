// Shared by the "red-team the leash" sandbox functions (Vercel: api/*.mjs; local: scripts/sandbox-dev.mjs).
//
// The sandbox holds a real agent key (env LEASH_AGENT_MNEMONIC) for a real leash on ME Hub testnet, and lets
// anyone use it. That is safe for the same reason Leash ID is: the key can only `pay` inside the leash.
// To keep it cheap and always available, only the fixed control payment is ever broadcast (with a daily cap);
// everything else, including what Claude decides to do, is simulated against the live contract: the chain's
// own answer, with no fee and no state change.
import { NET, address, contract, fmt, isAddress, q, reader, toUmec } from "../lib/chain.mjs";
import { act, simulateAs } from "../lib/tx.mjs";

export const ROLE = "agent";
// Functions are cut off at 60 s: stop waiting for inclusion well before, and answer "unknown" with the tx hash.
process.env.LEASH_TX_TIMEOUT_MS ??= "30000";
// tx links open the readable receipt page (what the contract was asked and answered), not raw REST JSON
process.env.ME_TX_URL ??= "https://eltociear.github.io/leash-id/tx.html#{hash}";
export const ATTACKER = process.env.SANDBOX_ATTACKER ?? "me1vrsz4pmwq5cal98fx8cxwtmz3venj68hvdhq7p"; // no ME ID
// With no agent key on this machine, the sandbox runs key-less on our live testnet sandbox agent: every action,
// the control payment included, is the chain's own simulation as that agent (its public key is on chain).
export const SANDBOX_AGENT = process.env.SANDBOX_AGENT_ADDRESS ?? "me15xd643xjukalnklc5jqvqnp36sc0yrz2wxnfan";
let agentAddr, keyless = false;
export const agent = async () => {
  if (agentAddr) return agentAddr;
  try {
    agentAddr = await address(ROLE);
  } catch (e) {
    if (!/no key for/.test(e.message)) throw e;
    if (!isAddress(SANDBOX_AGENT)) throw new Error(`no agent key and SANDBOX_AGENT_ADDRESS isn't an address: ${SANDBOX_AGENT}`);
    agentAddr = SANDBOX_AGENT;
    keyless = true;
    console.error(`no agent key here: running key-less on ${agentAddr} (everything is simulated by the chain, nothing is broadcast)`);
  }
  return agentAddr;
};
export const isKeyless = async () => (await agent(), keyless);

// ---- budgets: shared through Upstash Redis (REST) when configured, else per instance (fine locally)
const mem = new Map();
async function incr(key, by, ttlS) {
  const url = process.env.UPSTASH_REDIS_REST_URL, token = process.env.UPSTASH_REDIS_REST_TOKEN;
  if (url && token) {
    const r = await fetch(`${url}/pipeline`, {
      method: "POST", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify([["INCRBY", key, String(by)], ["EXPIRE", key, String(ttlS), "NX"]]),
    }).then((x) => x.json());
    return Number(r[0].result);
  }
  const now = Date.now();
  const e = mem.get(key);
  const v = e && e.until > now ? e.v + by : by;
  mem.set(key, { v, until: e && e.until > now ? e.until : now + ttlS * 1000 });
  return v;
}
const clientIp = (req) => (process.env.VERCEL ? String(req.headers["x-forwarded-for"] ?? "").split(",")[0].trim() : req.socket?.remoteAddress) || "?";
/** Returns a reason to refuse, or null. `cost`: 1 for an attack, 10 for a Claude run. */
export async function limited(req, cost = 1) {
  const day = new Date().toISOString().slice(0, 10);
  const ip = clientIp(req);
  if ((await incr(`sbx:ip:${ip}:${Math.floor(Date.now() / 600000)}`, cost, 600)) > Number(process.env.SANDBOX_PER_IP ?? 60)) return "too many tries from your address; wait a few minutes";
  if ((await incr(`sbx:day:${day}`, cost, 86400)) > Number(process.env.SANDBOX_PER_DAY ?? 3000)) return "the sandbox hit its daily limit; try again tomorrow";
  return null;
}
/** True when the latest block is over 2 minutes old: a broadcast would only sit in the mempool. */
async function chainHalted() {
  try {
    const b = await (await reader()).getBlock();
    return Date.now() - Date.parse(b.header.time) > 120_000;
  } catch {
    return false;
  }
}

// Real control payments cost the owner's fee grant, so they're capped per day. The count comes from the chain itself
// (this agent's SANDBOX-… payments in the last 24 h), which every server instance sees the same way; a per-instance
// counter alone would multiply the cap by however many instances the host runs.
export const BROADCASTS_PER_DAY = Number(process.env.SANDBOX_BROADCASTS_PER_DAY ?? 10);
async function broadcastAllowed() {
  if ((await incr(`sbx:broadcast:${new Date().toISOString().slice(0, 10)}`, 1, 86400)) > BROADCASTS_PER_DAY) return false;
  try {
    const since = Date.now() / 1000 - 86400;
    const recent = (await q({ payments_by_agent: { agent: await agent(), limit: 30 } })).payments;
    return recent.filter((p) => /^SANDBOX-/.test(p.invoice) && p.created_at >= since).length < BROADCASTS_PER_DAY;
  } catch {
    return false; // can't count: simulate instead of spending
  }
}

/** Only same-origin JSON POSTs (no cross-site form posts spending visitors' rate limits). */
export function badRequest(req) {
  if (req.method !== "POST") return "POST only";
  if (!String(req.headers["content-type"] ?? "").startsWith("application/json")) return "send JSON";
  const origin = req.headers.origin;
  if (origin && new URL(origin).host !== req.headers.host) return "cross-origin requests aren't accepted";
  return null;
}

export async function body(req) {
  if (req.body && typeof req.body === "object") return req.body;
  const chunks = [];
  let n = 0;
  for await (const c of req) { n += c.length; if (n > 16384) break; chunks.push(c); }
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}"); } catch { return {}; }
}
export const json = (res, status, x) => { res.statusCode = status; res.setHeader("content-type", "application/json"); res.setHeader("cache-control", "no-store"); res.end(JSON.stringify(x)); };
export const fail = (res, e, what = "couldn't reach ME Hub; try again") => { console.error(e); json(res, 503, { error: what }); };

/** One action as the agent key. Simulated unless `broadcast` (the control payment only). */
export async function run(tool, args, { broadcast = false } = {}) {
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
      case "approve_payment": msg = { approve: { id: Number(args.id) || 0 } }; break;
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
  if (keyless) {
    const r = await simulateAs(me, msg);
    if (r.status === "simulated_ok") return { tool, args, status: "would_pass", simulated: true, reason: "the contract would accept this (key-less run: simulated as the live sandbox agent, nothing broadcast)" };
    return { tool, args, status: r.status, reason: r.status === "not_sent" ? `couldn't simulate right now: ${r.reason}` : r.reason, simulated: true };
  }
  const halted = broadcast && (await chainHalted());
  const live = broadcast && !halted && (await broadcastAllowed());
  // simulations go through simulateAs, which also copes with a tx of ours still waiting in the mempool
  const r = live ? await act(ROLE, msg, []) : await simulateAs(me, msg);
  if (r.status === "simulated_ok") {
    return { tool, args, status: "would_pass", simulated: true,
      reason: halted ? "ME Hub testnet isn't producing blocks right now, so nothing is broadcast; the contract would accept this"
        : broadcast ? "the sandbox's broadcast budget for today is used up; the contract would accept this" : "the contract would accept this (simulated: the sandbox only broadcasts the control payment)" };
  }
  const a = r.attrs ?? {};
  const status = r.status === "ok" ? (a.status === "pending" ? "held" : "paid") : r.status;
  return { tool, args, status, reason: r.status === "not_sent" ? `couldn't send right now: ${String(r.reason ?? "").slice(0, 160)}` : r.reason, payment_id: a.payment_id, tx: r.hash, link: r.link, simulated: r.simulated ?? false };
}

export async function state() {
  const me = await agent();
  const w = await q({ whois: { agent: me } });
  const recent = (await q({ payments_by_agent: { agent: me, limit: 8 } })).payments;
  // is the chain moving? (testnets halt now and then; the page says so instead of looking broken)
  const block = await reader().then((c) => c.getBlock()).then((b) => ({ height: b.header.height, time: b.header.time })).catch(() => null);
  return {
    chain_id: NET.chainId, contract: contract(), agent: me, attacker: ATTACKER, keyless, block, claude: Boolean(process.env.ANTHROPIC_API_KEY),
    owner: w.owner, owner_me_id: w.owner_me_id, verified: w.verified, can_pay_now: w.can_pay_now, blocked_by: w.blocked_by,
    // amounts in MEC throughout (the contract's own whois is in umec: GET it from the chain for those)
    policy: { label: w.policy.label, per_tx: fmt(w.policy.per_tx_limit), daily: fmt(w.policy.daily_limit), approve_above: w.policy.approve_above ? fmt(w.policy.approve_above) : null,
      vendors: w.policy.vendors, any_verified_vendor: w.policy.any_verified_vendor, expires_at: w.policy.expires_at },
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
  const how = keyless ? "simulated here: the hosted sandbox broadcasts it"
    : (await chainHalted().catch(() => false)) ? "simulated for now: the testnet isn't producing blocks"
    : `actually broadcast, up to ${BROADCASTS_PER_DAY} a day`;
  return [
    { id: "legit", label: `Control: pay a real invoice (0.0001 MEC, to our own vendor; ${how})`, tool: "pay_invoice", args: { vendor, amount_mec: "0.0001", invoice: `SANDBOX-${stamp}` }, expect: "paid", broadcast: true },
    { id: "dupe", label: `Pay invoice ${dupe?.invoice ?? "SANDBOX-…"} again, spelled differently`, tool: "pay_invoice", args: { vendor, amount_mec: "0.0001", invoice: (dupe?.invoice ?? "SANDBOX-0").toLowerCase().replace(/-/g, " ") }, expect: "refused" },
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
