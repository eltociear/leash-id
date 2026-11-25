#!/usr/bin/env node
// The testnet records, checked against the chain itself. For every transaction a runs/testnet-* record links to,
// fetch it from ME Hub testnet and confirm that what the record says happened is what the chain says happened:
//   - "ok" / PAID / HELD lines → the tx succeeded (code 0)
//   - REFUSED lines → the tx failed, and the chain's own log carries the contract's reason
//   - every contract call went to the deployed Leash ID contract (deployment.json)
// So the records can't have been edited after the fact without this failing.
//   node scripts/check-chain.mjs [runs-dir]        (needs network; read-only, no keys)
import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2] ?? new URL("../runs", import.meta.url).pathname;
const REST = process.env.ME_REST_TESTNET ?? "https://beta-hub-1317.explorer-testnet.me";
const CONTRACT = JSON.parse(fs.readFileSync(new URL("../deployment.json", import.meta.url), "utf8")).contract;

const files = fs.readdirSync(dir).filter((f) => /^testnet-.*\.(txt|jsonl)$/.test(f)).sort();
// What the record claims, per hash: the nearest verdict word before the link.
const claims = new Map();
for (const f of files) {
  let text = fs.readFileSync(path.join(dir, f), "utf8");
  if (f.endsWith(".jsonl")) text = text.replace(/\\\\n|\\n/g, "\n").replace(/\\"/g, '"');
  for (const m of text.matchAll(/txs\/([0-9A-F]{64})/g)) {
    const before = text.slice(Math.max(0, m.index - 600), m.index);
    const verdicts = [...before.matchAll(/(REFUSED|PAID|HELD|^ok\s|^DONE\s)/gm)];
    const v = verdicts.pop();
    if (!v) continue;
    const refused = v[1] === "REFUSED";
    const reason = refused ? (before.slice(v.index).match(/REFUSED[^:\n]*: (.*?)(?:\s+https?:|\n|$)/)?.[1] ?? "") : "";
    if (!claims.has(m[1])) claims.set(m[1], { file: f, refused, reason });
  }
}

const get = async (h) => {
  for (let i = 0; i < 4; i++) {
    const r = await fetch(`${REST}/cosmos/tx/v1beta1/txs/${h}`).catch(() => null);
    if (r?.ok) return (await r.json()).tx_response;
    if (r?.status === 404 || r?.status === 400) return null;
    await new Promise((ok) => setTimeout(ok, 800 * (i + 1)));
  }
  throw new Error(`couldn't read ${h} from ${REST}`);
};

let bad = 0, n = 0;
const perFile = {};
for (const [h, c] of claims) {
  const t = await get(h);
  n++;
  const problems = [];
  if (!t) problems.push("not on chain");
  else {
    if (c.refused !== (t.code !== 0)) problems.push(`record says ${c.refused ? "refused" : "succeeded"}, chain says code ${t.code}`);
    // The CLI shows amounts in MEC; the contract logs them in umec (1 MEC = 1e8 umec). Compare in umec.
    const umec = (x) => x.replace(/(\d+(?:\.\d+)?) MEC/g, (_, v) => String(Math.round(Number(v) * 1e8)));
    if (c.refused && c.reason && !t.raw_log.includes(umec(c.reason)))
      problems.push(`reason "${c.reason}" not in the chain's log: ${t.raw_log.slice(0, 160)}`);
    const contracts = (t.tx?.body?.messages ?? []).filter((m) => m.contract).map((m) => m.contract);
    if (contracts.some((x) => x !== CONTRACT)) problems.push(`calls another contract: ${contracts.join(", ")}`);
  }
  perFile[c.file] ??= { n: 0, bad: 0 };
  perFile[c.file].n++;
  if (problems.length) { bad++; perFile[c.file].bad++; console.log(`FAIL  ${h.slice(0, 12)}… (${c.file}): ${problems.join("; ")}`); }
}
for (const [f, s] of Object.entries(perFile)) console.log(`${s.bad ? "FAIL" : "ok  "}  ${f}: ${s.n - s.bad}/${s.n} transactions match the chain`);
console.log(bad ? `\n${bad} of ${n} transactions don't match` : `\nall ${n} linked testnet transactions match what the records say`);
process.exit(bad || n === 0 ? 1 : 0);
