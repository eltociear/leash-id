// Offline proof that a transaction was in a ME Hub block, and what the chain made of it. No network, no trust in us:
//   tx bytes ──sha256──► the tx hash the records cite
//   tx hash  ──merkle proof──► header(H).data_hash ──► header(H) hashes to the block ID validators signed
//   (code, gas) of every tx in H ──merkle──► header(H+1).last_results_hash, and header(H+1).last_block_id = header(H)
// So a snapshot taken while the testnet ran (runs/chain-snapshot.json) still proves each record if the testnet is
// later reset. What it can't prove offline: the log text (CometBFT doesn't hash it) and that the signatures are by
// the real validator set (they're kept, with the validator set, for anyone who wants to check that too).
import { createHash } from "node:crypto";

const sha = (...b) => createHash("sha256").update(Buffer.concat(b.map((x) => Buffer.from(x)))).digest();
const leaf = (b) => sha([0], b);
const inner = (l, r) => sha([1], l, r);
const split = (n) => { let k = 1; while (k * 2 < n) k *= 2; return k; };
/** RFC 6962 merkle root, as CometBFT computes it. */
export function merkleRoot(items) {
  if (items.length === 0) return sha();
  if (items.length === 1) return leaf(items[0]);
  const k = split(items.length);
  return inner(merkleRoot(items.slice(0, k)), merkleRoot(items.slice(k)));
}
/** Root from a leaf hash and its aunts (CometBFT's computeHashFromAunts). */
export function rootFromAunts(index, total, leafHash, aunts) {
  if (total === 1) return aunts.length ? null : leafHash;
  if (!aunts.length) return null;
  const k = split(total), last = aunts[aunts.length - 1], rest = aunts.slice(0, -1);
  if (index < k) { const l = rootFromAunts(index, k, leafHash, rest); return l && inner(l, last); }
  const r = rootFromAunts(index - k, total - k, leafHash, rest);
  return r && inner(last, r);
}

// --- minimal protobuf encoding (proto3: zero values are omitted) ---
const varint = (n) => { n = BigInt(n); const o = []; do { let b = Number(n & 0x7fn); n >>= 7n; if (n) b |= 0x80; o.push(b); } while (n); return Buffer.from(o); };
const key = (f, wt) => varint((f << 3) | wt);
const fVarint = (f, n) => (BigInt(n) === 0n ? Buffer.alloc(0) : Buffer.concat([key(f, 0), varint(BigInt.asUintN(64, BigInt(n)))]));
const fBytes = (f, b) => (b.length ? Buffer.concat([key(f, 2), varint(b.length), b]) : Buffer.alloc(0));
const fMsg = (f, b) => Buffer.concat([key(f, 2), varint(b.length), b]); // a set sub-message is always written
const hex = (s) => Buffer.from(s ?? "", "hex");

function timestamp(rfc3339) {
  const m = rfc3339.match(/^(.*?)(?:\.(\d+))?Z$/);
  const secs = Math.floor(Date.parse(m[1] + "Z") / 1000);
  const nanos = Number((m[2] ?? "").padEnd(9, "0").slice(0, 9) || 0);
  return Buffer.concat([fVarint(1, secs), fVarint(2, nanos)]);
}
const blockId = (b) => Buffer.concat([fBytes(1, hex(b.hash)), fMsg(2, Buffer.concat([fVarint(1, b.parts?.total ?? 0), fBytes(2, hex(b.parts?.hash))]))]);

/** The block ID hash of a header as the RPC returns it (`/commit` → signed_header.header). */
export function headerHash(h) {
  const wrapBytes = (b) => fBytes(1, b); // gogoproto BytesValue / StringValue
  const fields = [
    Buffer.concat([fVarint(1, h.version.block), fVarint(2, h.version.app ?? 0)]),
    wrapBytes(Buffer.from(h.chain_id)),
    fVarint(1, h.height),
    timestamp(h.time),
    blockId(h.last_block_id),
    ...["last_commit_hash", "data_hash", "validators_hash", "next_validators_hash", "consensus_hash", "app_hash",
      "last_results_hash", "evidence_hash", "proposer_address"].map((k) => wrapBytes(hex(h[k]))),
  ];
  return merkleRoot(fields).toString("hex").toUpperCase();
}

/** The deterministic part of a tx result that last_results_hash commits to. */
const resultBytes = (r) => Buffer.concat([fVarint(1, r.code ?? 0), fBytes(2, Buffer.from(r.data ?? "", "base64")), fVarint(5, r.gas_wanted ?? 0), fVarint(6, r.gas_used ?? 0)]);

/** Check one tx of a snapshot ({ txs: {HASH: {height, index, total, aunts, tx, code, log}}, blocks: {H: {header,
 *  block_id, results?}} }). Returns a list of problems (empty = proven). */
export function verifyTx(hash, snap) {
  const e = snap.txs[hash], b = snap.blocks[e?.height], n = snap.blocks[Number(e?.height) + 1];
  if (!e || !b || !n) return ["not in the snapshot"];
  const p = [];
  const tx = Buffer.from(e.tx, "base64");
  if (sha(tx).toString("hex").toUpperCase() !== hash) p.push("tx bytes don't hash to the cited hash");
  const root = rootFromAunts(Number(e.index), Number(e.total), leaf(sha(tx)), e.aunts.map((a) => Buffer.from(a, "base64")));
  if (!root || root.toString("hex").toUpperCase() !== b.header.data_hash) p.push("tx isn't in the block's data_hash");
  if (String(b.header.height) !== String(e.height)) p.push("header is for another height");
  if (headerHash(b.header) !== b.block_id) p.push("header doesn't hash to its block ID");
  // the outcome: every result in block H hashes to header(H+1).last_results_hash, and H+1 follows H
  if (headerHash(n.header) !== n.block_id) p.push("next header doesn't hash to its block ID");
  if (n.header.last_block_id.hash !== b.block_id) p.push("the next block doesn't follow this one");
  const r = b.results?.[Number(e.index)];
  if (!r) p.push("no result for the tx");
  else {
    if (merkleRoot(b.results.map(resultBytes)).toString("hex").toUpperCase() !== n.header.last_results_hash) p.push("results don't match the next block's last_results_hash");
    if ((r.code ?? 0) !== e.code) p.push(`stored code ${e.code} isn't the committed one (${r.code ?? 0})`);
  }
  return p;
}
