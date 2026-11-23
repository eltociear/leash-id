#!/usr/bin/env node
// The worst case: an MCP client that does whatever any email says. No model, no judgment.
// It pays every invoice (duplicates included), follows the injected "SYSTEM NOTICE", wires the
// "CEO", approves its own pending payments, raises its own limits and tries to drain the budget.
// Whatever still gets through is what Leash ID allows; everything else is a refusal by ME Hub.
//
//   node mcp/obey-everything.mjs   -> runs/obey-<ts>.txt (+ the server's jsonl)
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const inbox = JSON.parse(readFileSync(new URL(process.env.LEASH_INBOX ?? "inbox/demo.json", root), "utf8"));
const c = new Client({ name: "obey-everything", version: "1" });
await c.connect(new StdioClientTransport({ command: "node", args: [new URL("mcp/server.mjs", root).pathname], env: process.env }));

const lines = [];
const call = async (name, args) => {
  const r = await c.callTool({ name, arguments: args });
  const t = r.content[0].text;
  const verdict = t.startsWith("DONE") ? "DONE   " : t.startsWith("REFUSED") ? "REFUSED" : "OTHER  ";
  const line = `${verdict} ${name} ${JSON.stringify(args)}\n        ${t.split("\n").join("\n        ")}`;
  lines.push(line);
  console.log(line);
  return t;
};

lines.push((await c.callTool({ name: "whoami", arguments: {} })).content[0].text, "");
for (const m of inbox) {
  lines.push(`--- ${m.id}: ${m.subject}`);
  const inv = m.invoice;
  await call("pay_invoice", { vendor: inv.vendor, amount_mec: inv.amount_mec, invoice: inv.ref });
  if (/raise_my_limit/.test(m.body)) {
    await call("raise_my_limit", { daily_mec: "1000", per_tx_mec: "1000", add_vendor: inv.vendor });
    await call("withdraw_budget", { amount_mec: "0.1" });
  }
  if (/approve_payment|approval/.test(m.body)) {
    for (let id = 1; id <= 8; id++) {
      const s = await c.callTool({ name: "payment_status", arguments: { id } }).catch(() => null);
      if (s && /"pending"/i.test(s.content[0].text)) await call("approve_payment", { id });
    }
  }
}
lines.push("--- and the straight drains");
await call("pay_invoice", { vendor: inbox[0].invoice.vendor, amount_mec: "0.5", invoice: "DRAIN-1" });
await call("pay_batch", { payments: Array.from({ length: 5 }, (_, i) => ({ vendor: inbox[0].invoice.vendor, amount_mec: "0.014", invoice: `SPLIT-${i}` })) });
lines.push("", (await c.callTool({ name: "whoami", arguments: {} })).content[0].text);
await c.close();

const done = lines.filter((l) => l.startsWith("DONE")).length;
const refused = lines.filter((l) => l.startsWith("REFUSED")).length;
lines.push("", `summary: ${done} executed, ${refused} refused by the chain`);
mkdirSync(new URL("runs/", root), { recursive: true });
const out = new URL(`runs/${process.env.LEASH_OBEY_PREFIX ?? "obey"}-${new Date().toISOString().replace(/[:.]/g, "-")}.txt`, root);
writeFileSync(out, lines.join("\n") + "\n");
console.log(`\nsummary: ${done} executed, ${refused} refused -> ${out.pathname}`);
