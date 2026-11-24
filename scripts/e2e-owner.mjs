#!/usr/bin/env node
// End-to-end test of docs/owner.html against a local ME Hub node (after `npm run devnet`, node restarted with
// devnet/start.sh, and `node scripts/serve-docs.mjs` running). Drives a real browser through: sign in, change
// a leash, deposit, approve a held payment, revoke, resume, a refused withdraw and a rejected bad input.
//   CHROME=/path/to/chrome node scripts/e2e-owner.mjs
import puppeteer from "puppeteer-core";
import { execSync } from "node:child_process";

process.env.LEASH_DEPLOYMENT ??= "deployment.devnet.json"; // before lib/chain.mjs reads it
const { readKey, deployment } = await import("../lib/chain.mjs");
const C = deployment().contract;
const base = "http://localhost:18766";
const url = `${base}/owner.html?rpc=${base}/rpc&rest=${base}&contract=${C}&chain=mechain_2401-1&explorer=http://localhost:1317/cosmos/tx/v1beta1/txs/`;
const chrome = process.env.CHROME ?? "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary";
const b = await puppeteer.launch({ executablePath: chrome, headless: "new", args: process.env.CI ? ["--no-sandbox"] : [] });
const p = await b.newPage();
await p.setViewport({ width: 1280, height: 2000 });
const errs = [];
p.on("pageerror", (e) => errs.push(e.message));
p.on("console", (m) => m.type() === "error" && errs.push(m.text()));
let ok = true;
const expect = (cond, what, got) => { ok &&= cond; console.log(`${cond ? "ok  " : "FAIL"}  ${what}${cond ? "" : `: ${got}`}`); };
const press = async (sel) => { await p.waitForSelector(sel, { timeout: 30000 }); await new Promise((r) => setTimeout(r, 400)); await p.evaluate((s) => document.querySelector(s).click(), sel); };
const step = async (n) => {
  await p.waitForFunction((n) => document.querySelectorAll("#log > div").length >= n, { timeout: 60000 }, n);
  return p.$eval("#log", (e) => e.innerText.split("\n")[0]);
};

await p.goto(url, { waitUntil: "networkidle0" });
await p.click("#dev summary");
await p.type("#mn", readKey("owner"));
await p.click("#dev-go");
await p.waitForSelector("#agents details summary", { timeout: 30000 });
expect(await p.$eval("#mn", (e) => e.value === ""), "mnemonic cleared from the page after use");

await p.click("#agents details summary");
await p.$eval("#a0-day", (e) => (e.value = "0.2"));
await press("form[data-edit] button");
let l = await step(1); expect(/✓ update leash/.test(l), "change the 24h limit", l);

execSync(`. devnet/env.sh && node leash.mjs pay vendor 0.018 E2E-${Date.now()}`, { shell: "/bin/bash", stdio: "ignore" });
await p.type("#amt", "0.01");
await press("#dep");
l = await step(2); expect(/✓ deposit 0.01 MEC/.test(l), "deposit", l);
await press("[data-approve]");
l = await step(3); expect(/✓ approve #/.test(l), "approve the held payment", l);
await press("[data-revoke]");
l = await step(4); expect(/✓ revoke/.test(l), "revoke", l);
await press("[data-resume]");
l = await step(5); expect(/✓ resume/.test(l), "resume", l);
await p.type("#amt", "99999");
await press("#wd");
l = await step(6); expect(/✗ withdraw .*insufficient owner budget: [\d.]+ MEC available/.test(l), "withdrawing too much is refused, in MEC", l);
await p.type("#o-agent", "nope");
await press("#offer button");
l = await step(7); expect(/ME Hub address/.test(l), "a bad address is caught before signing", l);
// two signatures: the owner adds a co-approver in the console; a held payment needs both
const { address } = await import("../lib/chain.mjs");
const cfo = await address("cfo");
await p.$eval("#appr-list", (e, v) => (e.value = v), cfo);
await p.$eval("#appr-n", (e) => (e.value = "2"));
await press("#appr-go");
l = await step(8); expect(/✓ set 1 co-approver\(s\), 2 signature\(s\) needed/.test(l), "add a co-approver: 2 signatures needed", l);
execSync(`. devnet/env.sh && node leash.mjs pay vendor 0.018 E2E2-${Date.now()}`, { shell: "/bin/bash", stdio: "ignore" });
await p.type("#amt", "0.01");
await press("#dep");
await step(9);
await press("[data-approve]");
l = await step(10); expect(/✓ approve #/.test(l), "the owner signs", l);
await p.waitForFunction(() => /1\/2 signed/.test(document.getElementById("pending").innerText), { timeout: 30000 });
expect(true, "after one signature the payment still waits (1/2 signed)");
const c = await b.newPage();
c.on("pageerror", (e) => errs.push(e.message));
await c.setViewport({ width: 1280, height: 2000 });
await c.goto(url, { waitUntil: "networkidle0" });
await c.click("#dev summary");
await c.type("#mn", readKey("cfo"));
await c.click("#dev-go");
await c.waitForSelector("#co [data-approve]", { timeout: 30000 });
await new Promise((r) => setTimeout(r, 400));
await c.evaluate(() => document.querySelector("#co [data-approve]").click());
await c.waitForFunction(() => document.querySelectorAll("#log > div").length >= 1, { timeout: 60000 });
l = await c.$eval("#log", (e) => e.innerText.split("\n")[0]);
expect(/✓ approve #/.test(l), "the co-approver signs from their own console", l);
await c.waitForFunction(() => /Nothing waits on your signature/.test(document.getElementById("co").innerText), { timeout: 30000 });
expect(true, "the payment left the co-approver's queue: paid");
execSync(`. devnet/env.sh && node leash.mjs approvers --threshold 1`, { shell: "/bin/bash", stdio: "ignore" });
expect(errs.length === 0, "no errors in the page", errs.join(" | "));
await b.close();
console.log(ok ? "\nowner console: all good" : "\nowner console: FAILED");
process.exit(ok ? 0 : 1);
