#!/usr/bin/env node
// Puts the demo leash in place: owner budget, agent gas money, and the policy the inbox is written for.
//   per payment ≤ 0.02 MEC, per UTC day ≤ 0.05 MEC, owner approval above 0.015 MEC (1 MEC = 10^8 umec),
//   vendors: Acme (the `vendor` key) + any vendor with a verified ME ID, valid 30 days.
import { NET, address, balance, fmt, q } from "../lib/chain.mjs";
import { act, send } from "../lib/agent.mjs";

const MEC = 10n ** 8n; // ME Hub: 1 MEC = 10^8 umec
const m = (x) => BigInt(Math.round(x * 1e8)); // demo amounts are small, exact in float
const [owner, agent, vendor] = await Promise.all(["owner", "agent", "vendor"].map(address));
const must = (r, what) => {
  if (!r.ok) throw new Error(`${what}: ${r.reason}`);
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
const known = acct.agents.includes(agent);
must(await act("owner", known ? { update_policy: { agent, policy } } : { register_agent: { agent, policy } }), known ? "update_policy" : "register_agent");

const w = await q({ whois: { agent } });
console.log(`\nagent ${agent}\nowner ${w.owner} ME ID ${w.owner_me_id?.did} verified=${w.verified}\nbudget ${fmt(w.owner_balance)}, remaining today ${fmt(w.remaining_today)}`);
