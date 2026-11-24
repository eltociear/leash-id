// Offline tests for the JS side: amounts and how chain errors are read.
import { test } from "node:test";
import assert from "node:assert/strict";
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
