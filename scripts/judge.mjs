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
  ["The same txs, proven offline from the committed chain snapshot (survives a testnet reset)", "node", ["scripts/check-chain.mjs", "--offline"]],
];
const t0 = Date.now();
// Is the testnet moving? A halted chain doesn't fail any check (they read history), but it's worth knowing first.
const REST = process.env.ME_REST ?? "https://beta-hub-1317.explorer-testnet.me";
try {
  const b = (await (await fetch(`${REST}/cosmos/base/tendermint/v1beta1/blocks/latest`, { signal: AbortSignal.timeout(15000) })).json()).block.header;
  const age = Math.round((Date.now() - Date.parse(b.time)) / 1000);
  const ago = age < 120 ? `${age} s ago` : age < 7200 ? `${Math.round(age / 60)} min ago` : `${(age / 3600).toFixed(1)} h ago`;
  let why = "";
  if (age > 120) {
    // a halt at the governance-scheduled upgrade height is planned: name the release
    const plan = await fetch(`${REST}/cosmos/upgrade/v1beta1/current_plan`, { signal: AbortSignal.timeout(8000) })
      .then((r) => r.json()).then((x) => x.plan).catch(() => null);
    why = plan && Math.abs(Number(plan.height) - Number(b.height)) <= 1
      ? `  (NOT producing blocks: halted at the scheduled upgrade to ${plan.name}, voted by governance, until validators switch releases.` +
        ` The checks below read what is already on chain and still apply; CI re-runs the kill-switch, obey-every-order and agent-to-agent flows on a local ${plan.name} chain)`
      : "  (NOT producing blocks right now; the checks below read what is already on chain and still apply)";
  }
  console.log(`ME Hub testnet: latest block ${b.height}, ${ago}${why}\n`);
} catch (e) {
  console.log(`ME Hub testnet: couldn't read the latest block (${e.message}); the chain checks below may fail for that reason\n`);
}
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
  "\nNext: the contract's own 44 tests need Rust 1.81 (npm run test:contract); the tour is JUDGES.md.");
process.exit(bad.length ? 1 : 0);
