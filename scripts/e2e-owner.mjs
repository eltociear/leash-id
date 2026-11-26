#!/usr/bin/env node
// End-to-end test of docs/owner.html against a local ME Hub node (after `npm run devnet`, node restarted with
// devnet/start.sh, and `node scripts/serve-docs.mjs` running). Drives a real browser through: sign in, change
// a leash, deposit, approve a held payment, revoke, resume, a refused withdraw, a rejected bad input, two-signature
// approval from two browsers, and paying / no longer paying an agent's fees (x/feegrant).
//   CHROME=/path/to/chrome node scripts/e2e-owner.mjs
import puppeteer from "puppeteer-core";
import { execSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";

process.env.LEASH_DEPLOYMENT ??= "deployment.devnet.json"; // before lib/chain.mjs reads it
const { readKey, deployment } = await import("../lib/chain.mjs");
const C = deployment().contract;
const base = "http://localhost:18766";
// tx links open the readable receipt page (docs/tx.html), pointed at the same local node
const txPage = `${base}/tx.html?rest=${base}&contract=${C}&chain=mechain_2401-1#`;
const url = `${base}/owner.html?rpc=${base}/rpc&rest=${base}&contract=${C}&chain=mechain_2401-1&explorer=${encodeURIComponent(txPage)}`;
const chrome = process.env.CHROME ?? "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary";
const b = await puppeteer.launch({ executablePath: chrome, headless: "new", args: process.env.CI ? ["--no-sandbox"] : [] });
const p = await b.newPage();
await p.setViewport({ width: 1280, height: 2000 });
const errs = [];
p.on("pageerror", (e) => errs.push(e.message));
// A busy single-node devnet sometimes answers a read with 5xx; the pages repeat those reads, so the browser's own
// "Failed to load resource: ... 50x" line is noise, not a page error. Everything else counts.
const noise = (t) => /^Failed to load resource: the server responded with a status of 50[234]\b/.test(t);
p.on("console", (m) => m.type() === "error" && !noise(m.text()) && errs.push(m.text()));
let ok = true;
const expect = (cond, what, got) => { ok &&= cond; console.log(`${cond ? "ok  " : "FAIL"}  ${what}${cond ? "" : `: ${got}`}`); };
const press = async (sel) => { await p.waitForSelector(sel, { timeout: 30000 }); await new Promise((r) => setTimeout(r, 400)); await p.evaluate((s) => document.querySelector(s).click(), sel); };
const step = async (n) => {
  await p.waitForFunction((n) => document.querySelectorAll("#log > div").length >= n && !document.body.dataset.busy, { timeout: 60000, polling: 250 }, n)
    .catch(async (e) => { console.log(`FAIL  step ${n} never finished; page errors: ${errs.join(" | ") || "none"}; log: ${await p.$eval("#log", (x) => x.children.length + " lines, busy=" + document.body.dataset.busy)}`); throw e; });
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
{
  // the log's tx link opens docs/tx.html: the approval, read back from the chain
  const href = await p.$eval("#log > div a", (a) => a.href);
  const t = await b.newPage();
  t.on("pageerror", (e) => errs.push(`tx.html: ${e.message}`));
  await t.goto(href, { waitUntil: "networkidle0" });
  await t.waitForFunction(() => !document.body.innerText.includes("Reading ME Hub"), { timeout: 30000 });
  const txt = await t.$eval("#out", (e) => e.innerText);
  expect(href.startsWith(txPage) && /Executed/.test(txt) && /approve a held payment/.test(txt) && /What the contract recorded/.test(txt) && /status paid/.test(txt),
    "the approval's tx link opens a readable receipt (tx.html): executed, the payment recorded as paid", txt.slice(0, 300));
  await t.close();
}
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
// fee grant from the browser: pay the agent's fees, see it on the card, stop paying
await p.bringToFront(); // the co-approver's tab was in front, and background tabs don't run the waits' animation frames
const gasCard = async () => p.$eval("#agents", (e) => e.innerText);
await p.evaluate(() => [...document.querySelectorAll("#agents summary")].find((s) => /Pay its fees/.test(s.textContent))?.click());
await p.$eval("#a0-gl", (e) => (e.value = "0.005"));
await press("form[data-gas] button");
l = await step(11); expect(/✓ pay fees of .* up to 0\.005 MEC/.test(l), "grant the agent's fees from the console (x/feegrant)", l);
await p.waitForFunction(() => /you pay them: 0\.005 MEC left.*contract calls only/.test(document.getElementById("agents").innerText), { timeout: 30000 });
expect(true, "the card shows the fee grant: 0.005 MEC left, contract calls only");
await press("[data-ungas]");
l = await step(12); expect(/✓ stop paying fees of/.test(l), "stop paying the agent's fees", l);
await p.waitForFunction(() => /from its own wallet/.test(document.getElementById("agents").innerText), { timeout: 30000 });
expect(/from its own wallet/.test(await gasCard()), "the card is back to: fees from its own wallet", "");
execSync(`. devnet/env.sh && node leash.mjs approvers --threshold 1`, { shell: "/bin/bash", stdio: "ignore" });
expect(errs.length === 0, "no errors in the page", errs.join(" | "));

// read-only view: no wallet, any owner's console; everything that would sign stays disabled
const ro = await b.newPage();
await ro.goto(url + "#view=" + (await address("owner")), { waitUntil: "networkidle0" });
await ro.waitForFunction(() => document.querySelectorAll("#agents .card").length > 0 && !document.body.dataset.busy, { timeout: 60000, polling: 250 });
const roState = await ro.evaluate(() => ({
  banner: /View only/.test(document.getElementById("ro").innerText),
  live: [...document.querySelectorAll("#app button, #app input, #app textarea, #app select")].filter((e) => !e.disabled).map((e) => e.id || e.textContent.trim()),
}));
expect(roState.banner && JSON.stringify(roState.live) === JSON.stringify(["csv"]), "read-only view of the owner's console: every control that signs is disabled (CSV stays)", JSON.stringify(roState));
await ro.close();

// accessibility (axe-core, WCAG 2 A/AA + best practice) on the signed-in console and the verifier
const axeSrc = fs.readFileSync(createRequire(import.meta.url).resolve("axe-core/axe.min.js"), "utf8");
for (const [name, page] of [["owner console", p], ["verifier", await b.newPage()]]) {
  if (name === "verifier") await page.goto(url.replace("/owner.html", "/") + "#" + (await address("agent")), { waitUntil: "networkidle0" });
  await page.addScriptTag({ content: axeSrc });
  const r = await page.evaluate(async () => await window.axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "best-practice"] }));
  expect(r.violations.length === 0, `${name}: no accessibility violations (${r.passes.length} rules pass)`, r.violations.map((v) => `${v.id}×${v.nodes.length}`).join(", "));
}
await b.close();
console.log(ok ? "\nowner console: all good" : "\nowner console: FAILED");
process.exit(ok ? 0 : 1);
