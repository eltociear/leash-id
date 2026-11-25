// Offline tests for the JS side: amounts and how chain errors are read.
import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { fmt, isAddress, isNotFound, toUmec, checkMnemonic } from "../lib/chain.mjs";
import { contractReason, mecAmounts } from "../lib/tx.mjs";

test("MEC amounts are exact and strict", () => {
  assert.equal(toUmec("0.015"), "1500000");
  assert.equal(toUmec("1"), "100000000");
  assert.equal(toUmec(" 0.00000001 "), "1");
  assert.equal(toUmec("12345678901234567890.12345678"), "1234567890123456789012345678");
  for (const bad of ["", "0", "0.0", "-1", "1e3", "0.123456789", "1,5", ".5", "abc", "0x10", null, undefined]) {
    assert.throws(() => toUmec(bad), undefined, String(bad));
  }
  assert.equal(fmt("1500000"), "0.015 MEC");
  assert.equal(fmt("100000000"), "1 MEC");
  assert.equal(fmt("0"), "0 MEC");
  assert.equal(fmt(10n ** 30n + 1n), "10000000000000000000000.00000001 MEC");
});

test("addresses", () => {
  assert.ok(isAddress("me1m8tf94y2uxcmj4efk0tf5t4d335ge6dhgh9vam"));
  assert.ok(isAddress("me13xlavey3hpkj823z7y3gcq9d23ut7829yjjygz3whr2y2kzlprhssfqa2h"));
  for (const bad of ["vendor", "cosmos1m8tf94y2uxcmj4efk0tf5t4d335ge6dhgh9vam", "me1short", "me1M8TF94Y2UXCMJ4EFK0TF5T4D335GE6DHGH9VAM", "me1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"]) {
    assert.ok(!isAddress(bad), bad);
  }
});

test("contract refusals are told apart from everything else", () => {
  // as cosmjs reports a failed simulation
  const sim = "Query failed with (6): rpc error: code = Unknown desc = failed to execute message; message index: 0: over daily limit: 4900000 spent in the last 24h + 1400000 > 5000000: execute wasm contract failed [CosmWasm/wasmd@v0.43.0/x/wasm/keeper/keeper.go:395] With gas wanted: '18446744073709551615' and gas used: '246970' : unknown request";
  assert.equal(contractReason(sim), "over daily limit: 0.049 MEC spent in the last 24h + 0.014 MEC > 0.05 MEC");
  // as a failed tx's raw_log reads
  const log = "failed to execute message; message index: 0: invoice INV-1001 to this vendor was already handled (payment #1): execute wasm contract failed";
  assert.equal(contractReason(log), "invoice INV-1001 to this vendor was already handled (payment #1)");
  // not contract decisions
  assert.equal(contractReason("insufficient fees; got: 10umec required: 60000umec: insufficient fee"), null);
  assert.equal(contractReason("out of gas in location: wasm contract; gasWanted: 200000, gasUsed: 200366: out of gas"), null);
  assert.equal(contractReason("fetch failed"), null);
  assert.equal(mecAmounts("insufficient owner budget: 100000000 available, 150000000 needed"), "insufficient owner budget: 1 MEC available, 1.5 MEC needed");
  assert.equal(mecAmounts("agent revoked"), "agent revoked");
});

test("not-found answers are not network errors", () => {
  assert.ok(isNotFound(new Error("Query failed with (6): Generic error: not registered: me1x is not a leashed agent")));
  assert.ok(!isNotFound(new Error("fetch failed")));
  assert.ok(!isNotFound(new Error("Bad status on response: 502")));
});

test("mnemonics are checked without echoing them", () => {
  const words = Array(24).fill("abandon").join(" ");
  assert.equal(checkMnemonic(`  ${words}\n`, "x"), words);
  assert.throws(() => checkMnemonic('"abandon abandon"', "keys file line 3"), (e) => !e.message.includes("abandon") && e.message.includes("line 3"));
});

test("RPC failover: a dead first endpoint falls through to the next", async () => {
  const { spawnSync } = await import("node:child_process");
  const http = await import("node:http");
  // a fake CometBFT endpoint that answers /status like a node would
  const srv = http.createServer((req, res) => {
    let b = ""; req.on("data", (c) => (b += c)); req.on("end", () => {
      const id = (() => { try { return JSON.parse(b).id; } catch { return 0; } })();
      res.setHeader("content-type", "application/json");
      // a real CometBFT /status answer (captured from ME Hub testnet), renamed so we can tell it came from here
      const fx = JSON.parse(fs.readFileSync(new URL("./fixtures/comet-status.json", import.meta.url), "utf8"));
      res.end(JSON.stringify({ ...fx, id }));
    });
  });
  await new Promise((r) => srv.listen(0, "127.0.0.1", r));
  const live = `http://127.0.0.1:${srv.address().port}`;
  const code = `const c = await import(${JSON.stringify(new URL("../lib/chain.mjs", import.meta.url).href)}); const r = await c.reader(); console.log(await r.getChainId());`;
  const out = await new Promise((resolve) => {
    import("node:child_process").then(({ execFile }) => execFile(process.execPath, ["--input-type=module", "-e", code], { env: { ...process.env, ME_RPC: `http://127.0.0.1:1,${live}` } }, (e, so, se) => resolve({ e, so, se })));
  });
  srv.close();
  assert.equal(out.so.trim(), "failover-test", out.se);
});

test("retry: repeats dropped connections, never a contract's answer (even one that contains 502)", async () => {
  const { retry } = await import("../lib/chain.mjs");
  let n = 0;
  assert.equal(await retry(async () => { if (++n < 2) throw new Error("fetch failed"); return "ok"; }), "ok");
  assert.equal(n, 2);
  n = 0;
  await assert.rejects(retry(async () => { n++; throw new Error("over daily limit: 1502000 > 1000000: execute wasm contract failed"); }), /daily limit/);
  assert.equal(n, 1);
});
