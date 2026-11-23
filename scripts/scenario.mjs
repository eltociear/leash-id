#!/usr/bin/env node
// Puts the demo leash in place: owner budget, agent gas money, and the policy the inbox is written for.
//   per payment ≤ 2 MEC, per UTC day ≤ 5 MEC, owner approval above 1.5 MEC,
//   vendors: Acme (the `vendor` key) + any vendor with a verified ME ID, valid 30 days.
import { NET, address, balance, fmt, q } from "../lib/chain.mjs";
import { act, send } from "../lib/agent.mjs";

const MEC = 1_000_000n;
const [owner, agent, vendor] = await Promise.all(["owner", "agent", "vendor"].map(address));
const must = (r, what) => {
  if (!r.ok) throw new Error(`${what}: ${r.reason}`);
  console.log(`${what.padEnd(16)} ${r.link}`);
  return r;
};

const budget = BigInt(process.env.BUDGET_MEC ?? 10) * MEC;
const acct = await q({ owner: { owner } });
if (BigInt(acct.balance) - BigInt(acct.reserved) < budget) {
  must(await act("owner", { deposit: {} }, [{ denom: NET.denom, amount: String(budget) }]), "deposit");
}
if ((await balance(agent)) < MEC / 2n) must(await send("owner", agent, MEC), "agent gas");

const policy = {
  label: "ap-clerk",
  per_tx_limit: String(2n * MEC),
  daily_limit: String(5n * MEC),
  approve_above: String(1_500_000n),
  vendors: [vendor],
  any_verified_vendor: true,
  expires_at: Math.floor(Date.now() / 1000) + 30 * 86400,
};
const known = acct.agents.includes(agent);
must(await act("owner", known ? { update_policy: { agent, policy } } : { register_agent: { agent, policy } }), known ? "update_policy" : "register_agent");

const w = await q({ whois: { agent } });
console.log(`\nagent ${agent}\nowner ${w.owner} ME ID ${w.owner_me_id?.did} verified=${w.verified}\nbudget ${fmt(w.owner_balance)}, remaining today ${fmt(w.remaining_today)}`);
