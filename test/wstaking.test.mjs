import { test } from "node:test";
import assert from "node:assert/strict";
import { toHex } from "@cosmjs/encoding";
import { MsgDoFixedDeposit, depositIdOf, fixedDepositMsg } from "../lib/wstaking.mjs";

test("MsgDoFixedDeposit encodes the fields me-hub's tx.proto numbers (1 account, 2 principal, 4 term)", () => {
  const m = fixedDepositMsg("me1abc", 5_000_000, 1).value;
  const bytes = MsgDoFixedDeposit.encode(m).finish();
  // 0a 06 "me1abc" | 12 0f (0a 04 "umec" 12 07 "5000000") | 20 01
  assert.equal(toHex(bytes), "0a066d6531616263" + "120f" + "0a04756d6563" + "1207" + "35303030303030" + "2001");
  const back = MsgDoFixedDeposit.decode(bytes);
  assert.deepEqual(back, { account: "me1abc", principal: { denom: "umec", amount: "5000000" }, term: 1n });
});

test("the new deposit's id comes from the fixed_deposit event, not from another module's id", () => {
  const events = [
    { type: "message", attributes: [{ key: "id", value: "999" }] },
    { type: "fixed_deposit", attributes: [{ key: "region_id", value: "me_earth" }, { key: "id", value: "42" }] },
  ];
  assert.equal(depositIdOf(events), 42);
  assert.equal(depositIdOf([]), undefined);
});
