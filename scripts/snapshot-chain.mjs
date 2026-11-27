#!/usr/bin/env node
// Keep the testnet evidence even if the testnet is reset: for every transaction the records and docs cite, save its
// bytes, its merkle proof, its block's header and results, the next block's header, the commit signatures and the
// validator set into runs/chain-snapshot.json. `node scripts/check-chain.mjs --offline` then proves each record
// from that file alone (lib/txproof.mjs says exactly what that covers).
//   node scripts/snapshot-chain.mjs          (needs the testnet; read-only, no keys; keeps entries it can't refetch)
import fs from "node:fs";
import path from "node:path";
import { verifyTx } from "../lib/txproof.mjs";

const root = new URL("..", import.meta.url).pathname;
const RPC = (process.env.ME_RPC_TESTNET ?? "https://beta-hub-26657.explorer-testnet.me").replace(/\/$/, "");
const out = path.join(root, "runs/chain-snapshot.json");

// every 64-hex hash cited as a tx: run records (…/txs/H) and the docs' tx-page links (tx.html#H)
const hashes = new Set();
const scan = (f, re) => { for (const m of fs.readFileSync(f, "utf8").matchAll(re)) hashes.add(m[1]); };
for (const f of fs.readdirSync(path.join(root, "runs")).filter((f) => /^testnet-.*\.(txt|jsonl)$/.test(f))) scan(path.join(root, "runs", f), /txs\/([0-9A-F]{64})/g);
for (const f of [...fs.readdirSync(root).filter((f) => f.endsWith(".md")), ...fs.readdirSync(path.join(root, "docs")).filter((f) => f.endsWith(".html")).map((f) => "docs/" + f)])
  scan(path.join(root, f), /tx\.html#([0-9A-F]{64})/g);
const dep = JSON.parse(fs.readFileSync(path.join(root, "deployment.json"), "utf8"));
for (const v of Object.values(dep)) if (typeof v === "string" && /^[0-9A-F]{64}$/.test(v)) hashes.add(v);

const rpc = async (p) => {
  for (let i = 0; i < 4; i++) {
    const r = await fetch(RPC + p, { signal: AbortSignal.timeout(20000) }).then((x) => x.json()).catch(() => null);
    if (r?.result) return r.result;
    if (r?.error) return null; // e.g. tx not found
    await new Promise((ok) => setTimeout(ok, 800 * (i + 1)));
  }
  throw new Error(`couldn't read ${p} from ${RPC}`);
};

const snap = fs.existsSync(out) ? JSON.parse(fs.readFileSync(out, "utf8")) : { txs: {}, blocks: {} };
const block = async (h, withResults) => {
  if (snap.blocks[h] && (!withResults || snap.blocks[h].results)) return;
  const c = await rpc(`/commit?height=${h}`);
  const b = { header: c.signed_header.header, block_id: c.signed_header.commit.block_id.hash, signatures: c.signed_header.commit.signatures };
  if (withResults) {
    b.results = ((await rpc(`/block_results?height=${h}`)).txs_results ?? []).map((r) => ({ code: r.code ?? 0, data: r.data ?? "", gas_wanted: r.gas_wanted, gas_used: r.gas_used }));
    b.validators = (await rpc(`/validators?height=${h}&per_page=100`)).validators.map((v) => ({ address: v.address, pub_key: v.pub_key, voting_power: v.voting_power }));
  }
  snap.blocks[h] = b;
};

let added = 0, missing = [];
for (const h of [...hashes].sort()) {
  if (!snap.txs[h]) {
    const t = await rpc(`/tx?hash=0x${h}&prove=true`);
    if (!t) { missing.push(h); continue; }
    const { index, total, aunts } = t.proof.proof;
    snap.txs[h] = { height: t.height, index: Number(index), total: Number(total), aunts, tx: t.tx, code: t.tx_result.code ?? 0, log: t.tx_result.log };
    added++;
  }
  const e = snap.txs[h];
  await block(e.height, true);
  await block(Number(e.height) + 1, false);
}
snap.chain_id = Object.values(snap.blocks)[0]?.header.chain_id;
snap.rpc = RPC;
snap.taken_at = snap.taken_at ?? new Date().toISOString();
if (added) snap.updated_at = new Date().toISOString();
// sorted keys so re-runs give the same file
const sorted = (o) => Object.fromEntries(Object.keys(o).sort((a, b) => (/^\d+$/.test(a) ? a - b : a < b ? -1 : 1)).map((k) => [k, o[k]]));
const { txs, blocks, ...meta } = snap;
fs.writeFileSync(out, JSON.stringify({ ...meta, txs: sorted(txs), blocks: sorted(blocks) }, null, 1) + "\n");

const bad = Object.keys(snap.txs).filter((h) => verifyTx(h, snap).length);
console.log(`${Object.keys(snap.txs).length} txs in ${path.relative(root, out)} (${added} new), ${bad.length} failing their own proof` +
  (missing.length ? `; not found on chain: ${missing.join(", ")}` : ""));
process.exit(bad.length ? 1 : 0);
