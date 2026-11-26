// The send path (lib/tx.mjs) against a fake client: every way a send can end, and that an unconfirmed send is
// reported as UNKNOWN with the hash of the bytes that were broadcast, never as "not sent".
import { test } from "node:test";
import assert from "node:assert/strict";
import { sha256 } from "@cosmjs/crypto";
import { toHex } from "@cosmjs/encoding";
import { TxRaw } from "cosmjs-types/cosmos/tx/v1beta1/tx";

process.env.LEASH_TX_TIMEOUT_MS = "300";
const { sendNow } = await import("../lib/tx.mjs");

const raw = { bodyBytes: new Uint8Array([1, 2, 3]), authInfoBytes: new Uint8Array([4, 5]), signatures: [new Uint8Array([6])] };
const HASH = toHex(sha256(TxRaw.encode(raw).finish())).toUpperCase();
const refusal = "Query failed with (6): rpc error: code = Unknown desc = failed to execute message; message index: 0: unauthorized: only the owner can do this: execute wasm contract failed";
const fake = (o = {}) => ({
  addr: "me1x",
  client: {
    simulate: o.simulate ?? (async () => 100_000),
    sign: async () => raw,
    broadcastTxSync: o.broadcast ?? (async () => HASH),
    getTx: o.getTx ?? (async () => null),
  },
});

test("a contract refusal in simulation: refused, nothing sent, no hash", async () => {
  const r = await sendNow(fake({ simulate: async () => { throw new Error(refusal); } }), [], "m", false);
  assert.equal(r.status, "refused");
  assert.equal(r.reason, "unauthorized: only the owner can do this");
  assert.equal(r.hash, undefined);
});

test("dry run stops after simulation", async () => {
  const r = await sendNow(fake({ broadcast: async () => assert.fail("must not broadcast") }), [], "m", true);
  assert.equal(r.status, "simulated_ok");
});

test("the connection drops during broadcast: UNKNOWN, with the hash of the signed bytes", async () => {
  const r = await sendNow(fake({ broadcast: async () => { throw new Error("socket hang up"); } }), [], "m", false);
  assert.equal(r.status, "unknown");
  assert.equal(r.hash, HASH);
});

test("broadcast accepted but not seen in time: UNKNOWN, with the hash", async () => {
  const r = await sendNow(fake(), [], "m", false);
  assert.equal(r.status, "unknown");
  assert.equal(r.hash, HASH);
  assert.match(r.link, new RegExp(HASH));
});

test("CheckTx rejection (e.g. fee too low): not sent, safe to retry", async () => {
  const e = Object.assign(new Error("Broadcasting transaction failed with code 13 (codespace: sdk). Log: insufficient fee"), { code: 13, codespace: "sdk" });
  const r = await sendNow(fake({ broadcast: async () => { throw e; } }), [], "m", false);
  assert.equal(r.status, "not_sent");
  assert.match(r.reason, /fee is too low or nobody pays it/);
});

test("included: ok with the contract's attributes; out of gas: failed, fee charged", async () => {
  const ok = await sendNow(fake({ getTx: async () => ({ code: 0, gasUsed: 90_000, height: 7, events: [{ type: "wasm", attributes: [{ key: "action", value: "pay" }, { key: "_contract_address", value: "me1c" }] }] }) }), [], "m", false);
  assert.equal(ok.status, "ok");
  assert.deepEqual(ok.attrs, { action: "pay" });
  assert.equal(ok.hash, HASH);
  const oog = await sendNow(fake({ getTx: async () => ({ code: 11, gasUsed: 140_000, height: 8, rawLog: "out of gas in location: wasm contract" }) }), [], "m", false);
  assert.equal(oog.status, "failed");
  assert.match(oog.reason, /out of gas/);
});

test("the chain is halted: not sent (nothing broadcast), and the reason says so", async () => {
  const old = { header: { height: 5523300, time: new Date(Date.now() - 8 * 3600e3).toISOString() } };
  const f = fake({ broadcast: async () => assert.fail("must not broadcast into a halted chain") });
  f.client.getBlock = async () => old;
  const r = await sendNow(f, [], "m", false);
  assert.equal(r.status, "not_sent");
  assert.match(r.reason, /isn't producing blocks .*#5523300.*8\.0 h old.*safe to retry/);
  // a fresh block, or a failing block query, doesn't get in the way
  f.client.getBlock = async () => ({ header: { height: 9, time: new Date().toISOString() } });
  f.client.broadcastTxSync = async () => HASH;
  assert.equal((await sendNow(f, [], "m", false)).status, "unknown");
  f.client.getBlock = async () => { throw new Error("fetch failed"); };
  assert.equal((await sendNow(f, [], "m", false)).status, "unknown");
});

test("CheckTx 'tx already in mempool' (sdk 19): UNKNOWN, those bytes reached a node", async () => {
  const e = Object.assign(new Error("Broadcasting transaction failed with code 19 (codespace: sdk). Log: tx already in mempool"), { code: 19, codespace: "sdk" });
  const r = await sendNow(fake({ broadcast: async () => { throw e; } }), [], "m", false);
  assert.equal(r.status, "unknown");
  assert.equal(r.hash, HASH);
});
