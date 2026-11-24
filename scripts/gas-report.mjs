#!/usr/bin/env node
// Measures gas for each Leash ID operation on the current node (run on a fresh devnet after scenario.mjs).
// Prints a markdown table; the fee column uses ME_GAS_PRICE (0.1 umec/gas on testnet).
//   source devnet/env.sh && node scripts/gas-report.mjs > docs/GAS.md
import { NET, address, fmt, q } from "../lib/chain.mjs";
import { act } from "../lib/tx.mjs";

const price = Number(NET.gasPrice.replace(/[a-z]+$/i, ""));
const [owner, vendor, shop, merchant] = await Promise.all(["owner", "vendor", "shop", "merchant"].map((r) => address(r)));
const agent = await address("agent");
const stamp = Date.now().toString(36);
const rows = [];
const measure = async (what, role, msg, funds = []) => {
  const r = await act(role, msg, funds);
  if (r.status !== "ok") throw new Error(`${what}: ${r.status} ${r.reason}`);
  rows.push([what, r.gasUsed]);
  return r;
};
const w = await q({ whois: { agent } });
await measure("deposit", "owner", { deposit: {} }, [{ denom: NET.denom, amount: "1000000" }]);
await measure("update_policy", "owner", { update_policy: { agent, policy: { ...w.policy, daily_limit: "20000000" } } });
await measure("pay (allow-listed vendor, paid)", "agent", { pay: { vendor, amount: "100000", invoice: `G-${stamp}-1`, memo: null } });
await measure("pay (any-verified vendor: 2nd ME ID lookup)", "agent", { pay: { vendor: shop, amount: "100000", invoice: `G-${stamp}-2`, memo: null } });
const held = await measure("pay (held for approval)", "agent", { pay: { vendor, amount: "1800000", invoice: `G-${stamp}-3`, memo: null } });
await measure("approve (owner alone)", "owner", { approve: { id: Number(held.attrs.payment_id) } });
const held2 = await measure("pay (held, to reject)", "agent", { pay: { vendor, amount: "1600000", invoice: `G-${stamp}-4`, memo: null } });
await measure("reject", "owner", { reject: { id: Number(held2.attrs.payment_id) } });
await measure("pay_batch (5 items, one multi_send)", "agent", { pay_batch: { payments: Array.from({ length: 5 }, (_, i) => ({ vendor, amount: "10000", invoice: `G-${stamp}-B${i}` })) } });
await measure("pay_batch (20 items, the maximum)", "agent", { pay_batch: { payments: Array.from({ length: 20 }, (_, i) => ({ vendor, amount: "10000", invoice: `G-${stamp}-T${i}` })) } });
await measure("register_sub_account", "vendor", { register_sub_account: { sub: merchant, main: shop } });
await measure("set_approvers (1 co-approver)", "owner", { set_approvers: { approvers: [await address("cfo")], threshold: 1 } });
await measure("revoke_agent", "owner", { revoke_agent: { agent } });
await measure("resume_agent", "owner", { resume_agent: { agent } });

console.log(`# Gas per operation\n\nMeasured on a local ME Hub node (me-hub ${process.env.ME_HUB_REF ?? "1e9eff8a"}, wasmd 0.43) with the canonical wasm, ${new Date().toISOString().slice(0, 10)}.`);
console.log(`Fee = gas × ${NET.gasPrice} (testnet's price). The chain charges for the gas a tx *requests*; our senders request simulated gas × 1.4, so the fee actually paid is about 1.4× the column.\n`);
console.log("| Operation | Gas used | Fee |\n|---|---:|---:|");
for (const [what, gas] of rows) console.log(`| ${what} | ${gas.toLocaleString("en-US")} | ${fmt(Math.ceil(gas * price))} |`);
console.log(`\nA refused payment costs nothing: it is caught in simulation and never broadcast.`);
console.log(`The largest message, a 20-item batch, stays far below the block gas limit (300,000,000 on our node; unlimited, -1, on testnet).`);
