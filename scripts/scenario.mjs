#!/usr/bin/env node
// Puts the demo leash in place: owner budget, agent gas money, and the policy the inbox is written for.
//   per payment ≤ 0.02 MEC, per rolling 24h ≤ 0.05 MEC, owner approval above 0.015 MEC (1 MEC = 10^8 umec),
//   vendors: Acme (the `vendor` key) + any vendor with a verified ME ID, valid 30 days.
// The leash takes two transactions: the owner offers it, the agent key accepts it.
import { NET, address, balance, fmt, isNotFound, q } from "../lib/chain.mjs";
import { act, send } from "../lib/tx.mjs";

const m = (x) => BigInt(Math.round(x * 1e8)); // demo amounts are small, exact in float
const [owner, agent, vendor] = await Promise.all(["owner", "agent", "vendor"].map((r) => address(r)));
const must = (r, what) => {
  if (r.status !== "ok") throw new Error(`${what}: ${r.status}: ${r.reason}`);
  console.log(`${what.padEnd(16)} ${r.link}`);
  return r;
};

const budget = m(Number(process.env.BUDGET_MEC ?? 0.5));
const acct = await q({ owner: { owner } });
if (BigInt(acct.balance) - BigInt(acct.reserved) < budget) {
  must(await act("owner", { deposit: {} }, [{ denom: NET.denom, amount: String(budget) }]), "deposit");
}
if ((await balance(agent)) < m(0.01)) must(await send("owner", agent, m(0.02)), "agent gas");

const policy = {
  label: "ap-clerk",
  per_tx_limit: String(m(0.02)),
  daily_limit: String(m(0.05)),
  approve_above: String(m(0.015)),
  vendors: [vendor],
  any_verified_vendor: true,
  expires_at: Math.floor(Date.now() / 1000) + 30 * 86400,
};
const current = await q({ whois: { agent } }).catch((e) => { if (isNotFound(e)) return null; throw e; });
if (current && current.owner === owner) {
  must(await act("owner", { update_policy: { agent, policy } }), "update_policy");
  if (current.revoked) must(await act("owner", { resume_agent: { agent } }), "resume_agent");
} else {
  if (current) throw new Error(`agent ${agent} is leashed by someone else (${current.owner}); it must leave_leash first`);
  must(await act("owner", { register_agent: { agent, policy } }), "offer_leash");
  must(await act("agent", { accept_leash: { owner } }), "accept_leash");
}

const w = await q({ whois: { agent } });
console.log(`\nagent ${agent}\nowner ${w.owner} ME ID ${w.owner_me_id?.did} verified=${w.verified} can_pay_now=${w.can_pay_now}` +
  `\nbudget ${fmt(w.owner_balance)}, remaining in 24h ${fmt(w.remaining_24h)}`);
