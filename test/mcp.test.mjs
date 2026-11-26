// The MCP server and CLI as a customer meets them: which tools exist, and when the server refuses to start.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SERVER = path.join(ROOT, "mcp/server.mjs");
const AGENT = "me1m8tf94y2uxcmj4efk0tf5t4d335ge6dhgh9vam";
const base = { PATH: process.env.PATH, HOME: os.tmpdir(), LEASH_HOME: fs.mkdtempSync(path.join(os.tmpdir(), "leash-home-")) };

async function tools(env) {
  const c = new Client({ name: "test", version: "1" });
  await c.connect(new StdioClientTransport({ command: process.execPath, args: [SERVER], env: { ...base, ...env }, stderr: "ignore" }));
  const names = (await c.listTools()).tools.map((t) => t.name).sort();
  await c.close();
  return names;
}

const SAFE = ["action_log", "check_payment", "find_invoice", "list_inbox", "lookup_me_id", "my_payments", "pay_batch", "pay_for_api", "pay_invoice", "payment_status", "read_message", "retry_api_answer", "verify_agent", "whoami"];

test("default tool set has no owner-only tools", async () => {
  assert.deepEqual(await tools({ LEASH_READONLY: "1", LEASH_AGENT_ADDRESS: AGENT }), SAFE);
});

test("honeypot tools only on request", async () => {
  const t = await tools({ LEASH_READONLY: "1", LEASH_AGENT_ADDRESS: AGENT, LEASH_HONEYPOT_TOOLS: "1" });
  for (const h of ["approve_payment", "withdraw_budget", "raise_my_limit", "send_mec"]) assert.ok(t.includes(h), h);
});

test("structured results carry the full message (clients may show only structuredContent)", async () => {
  const c = new Client({ name: "test", version: "1" });
  await c.connect(new StdioClientTransport({ command: process.execPath, args: [SERVER], env: { ...base, LEASH_READONLY: "1", LEASH_AGENT_ADDRESS: AGENT }, stderr: "ignore" }));
  const r = await c.callTool({ name: "pay_invoice", arguments: { vendor: AGENT, amount_mec: "0.01", invoice: "X" } });
  await c.close();
  assert.equal(r.isError, true);
  assert.equal(r.structuredContent.status, "not_sent");
  assert.match(r.structuredContent.message, /READ-ONLY/);
  assert.equal(r.structuredContent.message, r.content[0].text);
});

test("pay_for_api won't call local, private or non-https URLs", async () => {
  const c = new Client({ name: "test", version: "1" });
  await c.connect(new StdioClientTransport({ command: process.execPath, args: [SERVER], env: { ...base, LEASH_READONLY: "1", LEASH_AGENT_ADDRESS: AGENT }, stderr: "ignore" }));
  for (const url of ["http://localhost:8080/x", "https://127.0.0.1/x", "https://169.254.169.254/latest/meta-data/", "https://10.0.0.5/", "https://[::1]/", "file:///etc/passwd", "https://user:pw@example.com/"]) {
    const r = await c.callTool({ name: "pay_for_api", arguments: { url, max_mec: "0.01" } });
    assert.equal(r.isError, true, url);
    assert.match(r.content[0].text, /won't call/, url);
  }
  await c.close();
});

test("the agent never sees a demo inbox's answer key", async () => {
  const c = new Client({ name: "test", version: "1" });
  await c.connect(new StdioClientTransport({ command: process.execPath, args: [SERVER], env: { ...base, LEASH_READONLY: "1", LEASH_AGENT_ADDRESS: AGENT }, stderr: "ignore" }));
  for (const id of ["m1", "m3", "m6"]) {
    const r = await c.callTool({ name: "read_message", arguments: { id } });
    assert.match(r.content[0].text, /"invoice"/);
    assert.doesNotMatch(r.content[0].text, /"expect"|"pay"|"refuse"/, id);
  }
  await c.close();
});

test("read-only mode needs an agent address", () => {
  const r = spawnSync(process.execPath, [SERVER], { env: { ...base, LEASH_READONLY: "1" }, input: "" });
  assert.equal(r.status, 1);
  assert.match(r.stderr.toString(), /LEASH_AGENT_ADDRESS/);
});

test("the server refuses to run when another role's key sits in its environment (a model's shell inherits it)", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "leash-keys-"));
  fs.writeFileSync(path.join(dir, "agent.mnemonic"), (await DirectSecp256k1HdWallet.generate(24)).mnemonic, { mode: 0o600 });
  const r = spawnSync(process.execPath, [SERVER], { env: { ...base, LEASH_KEY_DIR: dir, LEASH_OWNER_MNEMONIC: (await DirectSecp256k1HdWallet.generate(24)).mnemonic }, input: "" });
  assert.equal(r.status, 1);
  assert.match(r.stderr.toString(), /env LEASH_OWNER_MNEMONIC/);
});

test("the server refuses to run next to readable owner keys", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "leash-keys-"));
  for (const r of ["agent", "owner"]) {
    fs.writeFileSync(path.join(dir, `${r}.mnemonic`), (await DirectSecp256k1HdWallet.generate(24)).mnemonic, { mode: 0o600 });
  }
  const env = { ...base, LEASH_KEY_DIR: dir };
  const r = spawnSync(process.execPath, [SERVER], { env, input: "" });
  assert.equal(r.status, 1);
  assert.match(r.stderr.toString(), /owner\.mnemonic/);
  assert.doesNotMatch(r.stderr.toString(), /abandon|\b[a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+ [a-z]+\b(?=.*\n?$)/);
  // alone with its key, it starts
  fs.rmSync(path.join(dir, "owner.mnemonic"));
  assert.deepEqual(await tools({ LEASH_KEY_DIR: dir }), SAFE);
});

test("CLI exit codes", () => {
  const cli = (...a) => spawnSync(process.execPath, [path.join(ROOT, "leash.mjs"), ...a], { env: base });
  assert.equal(cli("--help").status, 0);
  assert.equal(cli("help", "pay").status, 0);
  assert.equal(cli("nope").status, 2);
  assert.equal(cli("pay").status, 2);
  assert.equal(cli("deposit", "1e3").status, 1);
  assert.equal(cli("whois", "--bogus").status, 2);
});
