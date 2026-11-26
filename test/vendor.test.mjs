// The vendor paywall and payer proofs, offline: terms, invoice signing, proof checks and request validation.
import { test } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "leash-vendor-"));
for (const r of ["alice", "mallory"]) fs.writeFileSync(path.join(dir, `${r}.mnemonic`), (await DirectSecp256k1HdWallet.generate(24)).mnemonic, { mode: 0o600 });
process.env.LEASH_KEY_DIR = dir;
const { address, signProof, verifyProof } = await import("../lib/chain.mjs");
const { paywall, proofText } = await import("../lib/vendor.mjs");

test("payer proofs: only the signing key verifies, only for the exact text", async () => {
  const alice = await address("alice");
  const p = await signProof("alice", undefined, proofText("AB".repeat(32), "API-1"));
  assert.equal(p.signer, alice);
  assert.ok(await verifyProof({ ...p, signer: alice }, proofText("AB".repeat(32), "API-1")));
  assert.ok(!(await verifyProof({ ...p, signer: alice }, proofText("AB".repeat(32), "API-2"))), "other invoice");
  assert.ok(!(await verifyProof({ ...p, signer: await address("mallory") }, proofText("AB".repeat(32), "API-1"))), "claimed by someone else");
  assert.ok(!(await verifyProof({ signer: alice, pub_key: "garbage", signature: "x" }, "t")));
});

async function serve(handler) {
  const s = http.createServer(handler);
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  return { url: `http://127.0.0.1:${s.address().port}/`, close: () => s.close() };
}

test("paywall: 402 terms with a signed invoice, and requests without a proper proof are refused", async () => {
  const payTo = await address("alice");
  const srv = await serve(paywall({ payTo, priceUmec: "100000", secret: Buffer.alloc(32, 7) }, (req, res) => res.end("data")));
  try {
    const r = await fetch(srv.url);
    assert.equal(r.status, 402);
    const t = (await r.json()).leash_id;
    assert.equal(t.pay_to, payTo);
    assert.equal(t.amount, "100000");
    assert.match(t.invoice, /^API-[0-9A-Z]+-[0-9A-F]{10}-[0-9A-F]{16}$/);
    assert.ok(t.invoice.length <= 64);
    const r2 = await fetch(srv.url);
    assert.notEqual((await r2.json()).leash_id.invoice, t.invoice, "fresh invoice per request");
    assert.equal((await fetch(srv.url, { headers: { "x-leash-payment": "nope" } })).status, 400);
    assert.equal((await fetch(srv.url, { headers: { "x-leash-payment": "AB".repeat(32) } })).status, 401, "no proof");
    assert.equal((await fetch(srv.url, { headers: { "x-leash-payment": "AB".repeat(32), "x-leash-proof": "!!!" } })).status, 401, "unreadable proof");
  } finally {
    srv.close();
  }
});

test("paywall: a stranger's junk proof can't burn a payment; the payer's real proof redeems it once", async () => {
  const [payTo, alice] = [await address("mallory"), await address("alice")];
  const claimed = new Set();
  const store = { claim: async (h) => (claimed.has(h) ? false : (claimed.add(h), true)), release: async (h) => claimed.delete(h) };
  assert.throws(() => paywall({ payTo, priceUmec: "1", spent: {} }, () => {}), /claim/);
  let invoice;
  const TX = "CD".repeat(32);
  const srv = await serve(paywall({
    payTo, priceUmec: "100000", secret: Buffer.alloc(32, 7), spent: store,
    readTx: async (h) => (h === TX ? [{ vendor: payTo, status: "paid", kind: "created", invoice, amount: "100000", agent: alice, payment_id: "7" }] : null),
    checkPayer: async () => ({ ok: true }),
  }, (req, res, p) => res.end(`data for #${p.payment_id}`)));
  try {
    invoice = (await (await fetch(srv.url)).json()).leash_id.invoice;
    const junk = Buffer.from(JSON.stringify({ pub_key: "x", signature: "y" })).toString("base64");
    const r1 = await fetch(srv.url, { headers: { "x-leash-payment": TX.toLowerCase(), "x-leash-proof": junk } });
    assert.equal(r1.status, 403);
    assert.equal(claimed.size, 0, "a failed proof must not claim the payment");
    const p = await signProof("alice", undefined, proofText(TX, invoice));
    const real = Buffer.from(JSON.stringify({ pub_key: p.pub_key, signature: p.signature })).toString("base64");
    const r2 = await fetch(srv.url, { headers: { "x-leash-payment": TX, "x-leash-proof": real } });
    assert.equal(r2.status, 200);
    assert.equal(await r2.text(), "data for #7");
    const r3 = await fetch(srv.url, { headers: { "x-leash-payment": TX, "x-leash-proof": real } });
    assert.equal(r3.status, 402);
    assert.match((await r3.json()).error, /already used/);
  } finally { srv.close(); }
});
