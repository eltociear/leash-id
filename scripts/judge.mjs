#!/usr/bin/env node
// One command for judges: every check that needs no key, no wallet and no Rust, with a one-line verdict each.
//   npm run judge
// Read-only: it queries ME Hub testnet and reads the committed records; it never signs or sends anything.
import { spawnSync } from "node:child_process";

const checks = [
  ["The contract on testnet is this repo's wasm, with no admin", "node", ["scripts/verify.mjs"]],
  ["JS tests (amounts, send outcomes, MCP server, paywall), no chain needed", "node", ["--test", "test/"]],
  ["The committed run records say what the README and video claim", "node", ["scripts/check-runs.mjs"]],
  ["Every testnet tx those records link to says the same on chain", "node", ["scripts/check-chain.mjs"]],
];
const t0 = Date.now();
const results = [];
for (const [what, cmd, args] of checks) {
  if (process.stdout.isTTY) process.stdout.write(`… ${what}`);
  const s = Date.now();
  const r = spawnSync(cmd, args, { encoding: "utf8", env: { ...process.env, FORCE_COLOR: "0" } });
  const out = `${r.stdout ?? ""}${r.stderr ?? ""}`.trim();
  const n = (k) => out.match(new RegExp(`^ℹ ${k} (\\d+)`, "m"))?.[1];
  const last = n("pass") ? `${n("pass")} passed, ${n("fail")} failed` : (out.split("\n").filter((l) => l.trim()).slice(-1)[0] ?? "");
  const ok = r.status === 0 && !/FAIL|don't match|[1-9]\d* failed/.test(last);
  results.push({ ok, what, last, out });
  if (process.stdout.isTTY) process.stdout.write("\r\x1b[K");
  console.log(`${ok ? "PASS" : "FAIL"}  ${what}  (${((Date.now() - s) / 1000).toFixed(1)} s)\n      ${last.trim().slice(0, 160)}`);
}
const bad = results.filter((r) => !r.ok);
for (const r of bad) console.log(`\n--- ${r.what}\n${r.out.split("\n").slice(-25).join("\n")}`);
console.log(`\n${bad.length ? `${bad.length} of ${results.length} checks failed` : `all ${results.length} checks pass`} in ${((Date.now() - t0) / 1000).toFixed(0)} s` +
  "\nNext: the contract's own 39 tests need Rust 1.81 (npm run test:contract); the tour is JUDGES.md.");
process.exit(bad.length ? 1 : 0);
