// The chain snapshot proves the records offline, and any edit to it is caught.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { verifyTx, headerHash, merkleRoot } from "../lib/txproof.mjs";

const snap = JSON.parse(fs.readFileSync(new URL("../runs/chain-snapshot.json", import.meta.url), "utf8"));
const clone = () => structuredClone(snap);
const hashes = Object.keys(snap.txs);
const refused = hashes.find((h) => snap.txs[h].code !== 0);
const paid = hashes.find((h) => snap.txs[h].code === 0);

test("every tx in the snapshot is proven into its signed block", () => {
  assert.ok(hashes.length >= 40);
  for (const h of hashes) assert.deepEqual(verifyTx(h, snap), [], h);
});

test("an empty tree hashes like CometBFT's (sha256 of nothing)", () => {
  assert.equal(merkleRoot([]).toString("hex"), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
});

test("editing the outcome is caught: a refused tx can't be passed off as paid, or the reverse", () => {
  for (const h of [refused, paid]) {
    const s = clone();
    s.txs[h].code = s.txs[h].code ? 0 : 5;
    assert.match(verifyTx(h, s).join(), /isn't the committed one/);
    // ...and editing the block's results too breaks the next block's last_results_hash
    s.blocks[s.txs[h].height].results[s.txs[h].index].code = s.txs[h].code;
    assert.match(verifyTx(h, s).join(), /last_results_hash/);
  }
});

test("editing the tx bytes, the header or the proof is caught", () => {
  let s = clone();
  const b = Buffer.from(s.txs[paid].tx, "base64"); b[b.length - 1] ^= 1; s.txs[paid].tx = b.toString("base64");
  assert.match(verifyTx(paid, s).join(), /don't hash to the cited hash/);

  s = clone();
  s.blocks[s.txs[paid].height].header.time = "2026-11-20T00:00:00Z";
  assert.match(verifyTx(paid, s).join(), /header doesn't hash to its block ID/);

  s = clone();
  const H = s.txs[paid].height;
  s.blocks[H].header.data_hash = "00".repeat(32);
  s.blocks[H].block_id = headerHash(s.blocks[H].header); // a consistent but forged header: the next block disowns it
  assert.match(verifyTx(paid, s).join(), /isn't in the block's data_hash|doesn't follow/);

  s = clone();
  delete s.txs[paid];
  assert.deepEqual(verifyTx(paid, s), ["not in the snapshot"]);
});
