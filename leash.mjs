#!/usr/bin/env node
// Leash ID CLI: KYC-backed spending leashes for AI agents on ME Network.
import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";
import {
  KEY_DIR, LEGACY_KEYS_FILE, NET, ROOT, addrOf, address, balance, contract, contractInfo, deployment,
  fmt, isNotFound, keyFile, meIdRest, q, readKey, roles, saveDeployment, signer, toUmec,
} from "./lib/chain.mjs";
import { act, mecAmounts } from "./lib/tx.mjs";

const VERSION = JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf8")).version;

// name: [usage, summary, options]
const OPTS = {
  as: { type: "string" },
  json: { type: "boolean" },
  label: { type: "string" },
  "per-tx": { type: "string" },
  daily: { type: "string" },
  "approve-above": { type: "string" },
  vendors: { type: "string" },
  "add-vendor": { type: "string" },
  "remove-vendor": { type: "string" },
  "any-verified-vendor": { type: "string" },
  days: { type: "string" },
  memo: { type: "string" },
  agent: { type: "string" },
  owner: { type: "string" },
  limit: { type: "string" },
  before: { type: "string" },
  "min-kyc": { type: "string" },
  admin: { type: "string" },
  help: { type: "boolean", short: "h" },
};

const HELP = {
  keygen: ["keygen [role…]", "create keys (default: owner agent) in " + KEY_DIR + "; --split moves an old keys.env into per-role files"],
  addrs: ["addrs", "every local key: address, balance, ME ID"],
  meid: ["meid <role|address>", "an address's ME ID, from REST and through the contract"],
  info: ["info", "network, contract, code id, admin, version"],
  deploy: ["deploy [--as owner] [--min-kyc 2] [--admin none|<address>]", "store + instantiate the contract (default admin: none, i.e. immutable)"],
  deposit: ["deposit <MEC> [--as owner]", "escrow budget for your agents (needs a live ME ID)"],
  withdraw: ["withdraw <MEC> [--as owner]", "take back budget that isn't reserved for held payments"],
  offer: ["offer <agent> --per-tx <MEC> --daily <MEC> [--approve-above <MEC>] [--vendors a,b] [--any-verified-vendor yes] [--days 30] [--label name]", "offer a leash to an agent key (step 1 of 2)"],
  accept: ["accept <owner> [--as agent]", "the agent key accepts the owner's leash (step 2 of 2)"],
  leave: ["leave [--as agent]", "the agent key drops its leash"],
  "cancel-offer": ["cancel-offer <agent>", "withdraw an offer the agent hasn't accepted"],
  offers: ["offers <agent>", "leashes offered to an agent and not yet accepted"],
  policy: ["policy <agent> [--per-tx] [--daily] [--approve-above <MEC|none>] [--vendors a,b] [--add-vendor a] [--remove-vendor a] [--any-verified-vendor yes|no] [--days N] [--label]", "change only the given parts of a leash"],
  revoke: ["revoke <agent>", "stop an agent now; its held payments are rejected"],
  resume: ["resume <agent>", "lift a revoke"],
  pay: ["pay <vendor> <MEC> <invoice> [--memo text] [--as agent]", "pay as the agent"],
  batch: ["batch <vendor:MEC:invoice>… [--as agent]", "several payments in one multi_send"],
  check: ["check <vendor> <MEC> <invoice> [--agent agent]", "dry-run a payment against the on-chain rules"],
  pending: ["pending [owner]", "payments waiting for the owner's approval"],
  approve: ["approve <id> [--as owner]", "pay a held payment"],
  reject: ["reject <id> [--as owner]", "refuse a held payment (frees its invoice number)"],
  release: ["release <vendor> <invoice> [--as owner]", "let an already-handled invoice number be paid again"],
  whois: ["whois <agent>", "who stands behind an agent, its leash, and whether it can pay now"],
  owner: ["owner [owner]", "an owner's budget, reserved funds and agents"],
  history: ["history [--agent a | --owner o] [--limit 20] [--before id]", "payments, newest first"],
  invoice: ["invoice <vendor> <invoice> [--owner o]", "was this invoice already handled?"],
};

function usage(cmd) {
  if (cmd && HELP[cmd]) return `usage: leash ${HELP[cmd][0]}\n  ${HELP[cmd][1]}`;
  const w = Math.max(...Object.keys(HELP).map((k) => k.length));
  return `leash ${VERSION}: KYC-backed spending leashes for AI agents on ME Hub (${NET.chainId})\n\n` +
    Object.entries(HELP).map(([k, [, s]]) => `  ${k.padEnd(w)}  ${s}`).join("\n") +
    "\n\n  roles are key names in " + KEY_DIR + " (owner, agent, …) or any me1… address\n" +
    "  amounts are MEC (1 MEC = 10^8 umec), e.g. 0.015\n  leash help <command> for details";
}

class UsageError extends Error {}
const need = (v, what) => { if (v === undefined || v === "") throw new UsageError(`missing ${what}`); return v; };
const intArg = (v, what) => { if (!/^\d+$/.test(String(v ?? ""))) throw new UsageError(`${what} must be a whole number`); return Number(v); };
const yesNo = (v, what) => {
  if (/^(yes|true|1|on)$/i.test(v)) return true;
  if (/^(no|false|0|off)$/i.test(v)) return false;
  throw new UsageError(`${what} must be yes or no`);
};

let argv;
try {
  argv = parseArgs({ args: process.argv.slice(2), options: { ...OPTS, split: { type: "boolean" } }, allowPositionals: true, strict: true });
} catch (e) {
  console.error(`leash: ${e.message}\n\n${usage()}`);
  process.exit(2);
}
const { values: o, positionals } = argv;
const [cmd, ...args] = positionals;

const print = (x) => console.log(o.json ? JSON.stringify(x, null, 2) : x);
const when = (s) => new Date(s * 1000).toISOString().slice(0, 16).replace("T", " ") + " UTC";
const payLine = (p) => `#${String(p.id).padEnd(5)} ${p.status.padEnd(8)} ${fmt(p.amount).padStart(14)}  ${p.vendor}  ${JSON.stringify(p.invoice)}  ${when(p.created_at)}`;

/** Prints a send outcome; exit code 0 only when it went through. */
function outcome(r) {
  if (o.json) { console.log(JSON.stringify({ ...r, events: undefined }, null, 2)); return r.status === "ok" ? 0 : 1; }
  const a = r.attrs ?? {};
  switch (r.status) {
    case "ok":
      console.log(`ok   ${a.action ?? "done"}${a.status ? ` (${a.status})` : ""}${a.payment_id ? ` payment #${a.payment_id}` : ""}  ${r.link}`);
      console.log("     " + Object.entries(a).filter(([k]) => k !== "action").map(([k, v]) => `${k}=${v}`).join(" "));
      return 0;
    case "refused": console.error(`REFUSED by the contract: ${r.reason}${r.hash ? `  ${r.link}` : "  (simulation; nothing sent, no fee)"}`); return 1;
    case "unknown": console.error(`UNKNOWN: tx ${r.hash} was broadcast but not confirmed (${r.reason}). It may have gone through: ${r.link}`); return 3;
    case "failed": console.error(`FAILED on chain: ${r.reason}  ${r.link}`); return 1;
    default: console.error(`NOT SENT: ${r.reason}`); return 1;
  }
}

async function policyFrom(base) {
  const p = base ? { ...base } : {};
  if (!base) {
    need(o["per-tx"], "--per-tx");
    need(o.daily, "--daily");
    if (!o.vendors && o["any-verified-vendor"] === undefined) throw new UsageError("give --vendors and/or --any-verified-vendor yes");
  }
  if (o.label !== undefined) p.label = o.label;
  p.label ??= "agent";
  if (o["per-tx"] !== undefined) p.per_tx_limit = toUmec(o["per-tx"]);
  if (o.daily !== undefined) p.daily_limit = toUmec(o.daily);
  if (o["approve-above"] !== undefined) p.approve_above = /^none$/i.test(o["approve-above"]) ? null : toUmec(o["approve-above"]);
  p.approve_above ??= null;
  if (o.vendors !== undefined) p.vendors = await Promise.all(o.vendors.split(",").map((s) => s.trim()).filter(Boolean).map(addrOf));
  p.vendors ??= [];
  if (o["add-vendor"]) p.vendors = [...new Set([...p.vendors, await addrOf(o["add-vendor"])])];
  if (o["remove-vendor"]) { const r = await addrOf(o["remove-vendor"]); p.vendors = p.vendors.filter((v) => v !== r); }
  if (o["any-verified-vendor"] !== undefined) p.any_verified_vendor = yesNo(o["any-verified-vendor"], "--any-verified-vendor");
  p.any_verified_vendor ??= false;
  if (o.days !== undefined || !base) p.expires_at = Math.floor(Date.now() / 1000) + intArg(o.days ?? "30", "--days") * 86400;
  return p;
}

const as = (def) => o.as ?? def;
const send = async (role, msg, funds) => outcome(await act(role, msg, funds));

const cmds = {
  async keygen() {
    fs.mkdirSync(KEY_DIR, { recursive: true, mode: 0o700 });
    fs.chmodSync(KEY_DIR, 0o700);
    if (o.split) {
      if (!fs.existsSync(LEGACY_KEYS_FILE)) throw new UsageError(`nothing to split: ${LEGACY_KEYS_FILE} doesn't exist`);
      for (const r of roles()) {
        if (fs.existsSync(keyFile(r))) continue;
        fs.writeFileSync(keyFile(r), readKey(r) + "\n", { mode: 0o600 });
        console.log(`moved ${r} -> ${keyFile(r)}`);
      }
      console.log(`now delete ${LEGACY_KEYS_FILE} (it still holds every key in one file)`);
      return 0;
    }
    for (const r of args.length ? args : ["owner", "agent"]) {
      if (!/^[a-z0-9][a-z0-9_-]{0,31}$/.test(r)) throw new UsageError(`bad role name "${r}"`);
      const f = keyFile(r);
      if (!fs.existsSync(f)) {
        const w = await DirectSecp256k1HdWallet.generate(24, { prefix: NET.prefix });
        fs.writeFileSync(f, w.mnemonic + "\n", { mode: 0o600, flag: "wx" });
      }
      fs.chmodSync(f, 0o600);
      console.log(r.padEnd(8), await address(r), f);
    }
    return 0;
  },
  async addrs() {
    for (const r of roles()) {
      const a = await address(r);
      const id = await meIdRest(a).catch(() => undefined);
      const meid = id === undefined ? "ME ID: (couldn't reach ME Hub)" : id ? `ME ID ${id.did} ${id.status} ${id.kycLevel}` : "no ME ID";
      console.log(r.padEnd(8), a, fmt(await balance(a)).padStart(16), " ", meid);
    }
    return 0;
  },
  async meid() {
    const a = await addrOf(need(args[0], "role or address"));
    print({ address: a, rest: await meIdRest(a), contract: await q({ me_id: { address: a } }).catch((e) => `n/a: ${e.message}`) });
    return 0;
  },
  async info() {
    const c = contract();
    const ci = await contractInfo(c);
    const v = await q({ version: {} }).catch(() => null);
    print({ ...NET, contract: c, code_id: ci.code_id, creator: ci.creator, admin: ci.admin || "(none: immutable)", version: v?.version ?? "(pre-0.2)", config: await q({ config: {} }) });
    return 0;
  },
  async deploy() {
    const wasm = fs.readFileSync(path.join(ROOT, "artifacts/leash_id.wasm"));
    const { client, addr } = await signer(as("owner"));
    const adminOpt = o.admin ?? "none";
    const admin = adminOpt === "none" ? undefined : adminOpt === "self" ? addr : await addrOf(adminOpt);
    const up = await client.upload(addr, wasm, "auto", "leash-id");
    console.log("stored code", up.codeId, up.transactionHash);
    const init = await client.instantiate(addr, up.codeId,
      { min_kyc_level: intArg(o["min-kyc"] ?? "2", "--min-kyc"), denom: NET.denom }, `leash-id ${VERSION}`, "auto", admin ? { admin } : {});
    console.log("contract", init.contractAddress, init.transactionHash, admin ? `admin ${admin}` : "(no admin: immutable)");
    const prev = deployment();
    saveDeployment({
      chainId: NET.chainId, codeId: up.codeId, contract: init.contractAddress, version: VERSION, admin: admin ?? null,
      storeTx: up.transactionHash, instantiateTx: init.transactionHash,
      wasmSha256: (await import("node:crypto")).createHash("sha256").update(wasm).digest("hex"),
      deployedAt: new Date().toISOString(),
      superseded: prev.contract ? [{ codeId: prev.codeId, contract: prev.contract, version: prev.version ?? "0.1.0" }, ...(prev.superseded ?? [])] : prev.superseded,
    });
    return 0;
  },
  deposit: () => send(as("owner"), { deposit: {} }, [{ denom: NET.denom, amount: toUmec(need(args[0], "amount")) }]),
  withdraw: () => send(as("owner"), { withdraw: { amount: toUmec(need(args[0], "amount")) } }),
  async offer() {
    return send(as("owner"), { register_agent: { agent: await addrOf(need(args[0], "agent")), policy: await policyFrom() } });
  },
  accept: async () => send(as("agent"), { accept_leash: { owner: await addrOf(need(args[0], "owner")) } }),
  leave: () => send(as("agent"), { leave_leash: {} }),
  "cancel-offer": async () => send(as("owner"), { cancel_offer: { agent: await addrOf(need(args[0], "agent")) } }),
  async offers() { print(await q({ offers: { agent: await addrOf(need(args[0], "agent")) } })); return 0; },
  async policy() {
    const agent = await addrOf(need(args[0], "agent"));
    const cur = (await q({ whois: { agent } })).policy;
    return send(as("owner"), { update_policy: { agent, policy: await policyFrom(cur) } });
  },
  revoke: async () => send(as("owner"), { revoke_agent: { agent: await addrOf(need(args[0], "agent")) } }),
  resume: async () => send(as("owner"), { resume_agent: { agent: await addrOf(need(args[0], "agent")) } }),
  async pay() {
    const [vendor, amount, invoice] = args;
    return send(as("agent"), { pay: { vendor: await addrOf(need(vendor, "vendor")), amount: toUmec(need(amount, "amount")), invoice: need(invoice, "invoice"), memo: o.memo ?? null } });
  },
  async batch() {
    if (!args.length) throw new UsageError("give at least one vendor:MEC:invoice");
    const payments = await Promise.all(args.map(async (s) => {
      const [v, a, ...i] = s.split(":");
      if (!v || !a || !i.length) throw new UsageError(`"${s}" is not vendor:MEC:invoice`);
      return { vendor: await addrOf(v), amount: toUmec(a), invoice: i.join(":") };
    }));
    return send(as("agent"), { pay_batch: { payments } });
  },
  async check() {
    const [vendor, amount, invoice] = args;
    const r = await q({ check_pay: { agent: await addrOf(o.agent ?? "agent"), vendor: await addrOf(need(vendor, "vendor")), amount: toUmec(need(amount, "amount")), invoice: need(invoice, "invoice"), memo: o.memo ?? null } });
    print(o.json ? r : r.ok ? `would be accepted${r.needs_approval ? ", held for the owner's approval" : " and paid"}` : `would be refused: ${mecAmounts(r.reason)}`);
    return r.ok ? 0 : 1;
  },
  async pending() {
    const r = await q({ pending: { owner: await addrOf(args[0] ?? "owner"), limit: 100 } });
    print(o.json ? r : r.payments.map(payLine).join("\n") || "(nothing waiting)");
    return 0;
  },
  approve: () => send(as("owner"), { approve: { id: intArg(need(args[0], "payment id"), "payment id") } }),
  reject: () => send(as("owner"), { reject: { id: intArg(need(args[0], "payment id"), "payment id") } }),
  release: async () => send(as("owner"), { release_invoice: { vendor: await addrOf(need(args[0], "vendor")), invoice: need(args[1], "invoice") } }),
  async whois() {
    const agent = await addrOf(need(args[0] ?? "agent", "agent"));
    try {
      const w = await q({ whois: { agent } });
      if (o.json) return print(w), 0;
      const p = w.policy;
      console.log(`agent      ${w.agent} "${p.label}"\nowner      ${w.owner}\nME ID      ${w.owner_me_id ? `${w.owner_me_id.did} ${w.owner_me_id.active ? "active" : "INACTIVE"} KYC ${w.owner_me_id.kyc_level}` : "none"} (needs KYC ≥ ${w.min_kyc_level}) → verified ${w.verified}`);
      console.log(`can pay    ${w.can_pay_now ? "yes" : `NO: ${w.blocked_by}`}`);
      console.log(`limits     ≤ ${fmt(p.per_tx_limit)} per payment, ≤ ${fmt(p.daily_limit)} per 24h, approval above ${p.approve_above ? fmt(p.approve_above) : "never"}`);
      console.log(`24h spend  ${fmt(w.spent_24h)} (remaining ${fmt(w.remaining_24h)}); owner budget ${fmt(w.owner_balance)}; ${w.pending_count} held`);
      console.log(`vendors    ${p.vendors.join(", ") || "(none)"}${p.any_verified_vendor ? " + any verified ME ID holder" : ""}\nexpires    ${when(p.expires_at)}${w.revoked ? "  REVOKED" : ""}`);
      return 0;
    } catch (e) {
      if (isNotFound(e)) { console.log(`${agent} is not a Leash ID agent`); return 1; }
      throw e;
    }
  },
  async owner() { print(await q({ owner: { owner: await addrOf(args[0] ?? "owner") } })); return 0; },
  async history() {
    const limit = intArg(o.limit ?? "20", "--limit"), start_before = o.before ? intArg(o.before, "--before") : null;
    const r = o.owner
      ? await q({ payments_by_owner: { owner: await addrOf(o.owner), start_before, limit } })
      : await q({ payments_by_agent: { agent: await addrOf(o.agent ?? "agent"), start_before, limit } });
    print(o.json ? r : r.payments.map(payLine).join("\n") || "(no payments)");
    return 0;
  },
  async invoice() {
    const p = await q({ invoice: { owner: await addrOf(o.owner ?? "owner"), vendor: await addrOf(need(args[0], "vendor")), invoice: need(args[1], "invoice") } });
    print(o.json ? p : p ? `handled: ${payLine(p)}` : "not handled yet");
    return 0;
  },
};
// old name, kept for scripts written against 0.1
cmds.register = cmds.offer;
cmds.payments = cmds.history;

if (cmd === "help" || o.help || !cmd) {
  console.log(usage(cmd === "help" ? args[0] : cmd));
  process.exit(0);
}
if (!cmds[cmd]) {
  console.error(`leash: unknown command "${cmd}"\n\n${usage()}`);
  process.exit(2);
}
try {
  process.exit(await cmds[cmd]());
} catch (e) {
  if (e instanceof UsageError) { console.error(`leash ${cmd}: ${e.message}\n${usage(cmd)}`); process.exit(2); }
  console.error(`leash ${cmd}: ${e.message ?? e}`);
  process.exit(1);
}
