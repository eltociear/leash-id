#!/usr/bin/env node
// Leash ID CLI: KYC-backed spending leashes for AI agents on ME Network.
import fs from "node:fs";
import path from "node:path";
import { parseArgs } from "node:util";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";
import {
  KEY_DIR, LEGACY_KEYS_FILE, NET, ROOT, addrOf, address, balance, contract, contractInfo, deployment,
  feeGrant, fmt, isNotFound, keyFile, meIdRest, q, readKey, reader, roles, saveDeployment, signer, toUmec,
} from "./lib/chain.mjs";
import { act, grantGasMsg, mecAmounts, revokeGasMsg, sendTx } from "./lib/tx.mjs";

const VERSION = JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf8")).version;

// name: [usage, summary, options]
const OPTS = {
  as: { type: "string" },
  "key-file": { type: "string" },
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
  "fee-granter": { type: "string" },
  before: { type: "string" },
  "min-kyc": { type: "string" },
  admin: { type: "string" },
  webhook: { type: "string" },
  threshold: { type: "string" },
  from: { type: "string" },
  once: { type: "boolean" },
  csv: { type: "boolean" },
  interval: { type: "string" },
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
  history: ["history [--agent a | --owner o] [--limit 20] [--before id] [--csv]", "payments, newest first (--csv: every payment, for accounting)"],
  invoice: ["invoice <vendor> <invoice> [--owner o]", "was this invoice already handled?"],
  approvers: ["approvers <addr,…> --threshold N [--as owner]", "co-approvers for held payments: N signatures (yours included) release one; any one can reject"],
  "gas-grant": ["gas-grant <agent> --limit <MEC> [--days 30] [--as owner]", "pay an agent's fees from your wallet (contract calls only, capped), so the agent key can hold no MEC at all"],
  "gas-revoke": ["gas-revoke <agent> [--as owner]", "stop paying an agent's fees"],
  "link-sub": ["link-sub <sub-account> <main>", "record an EVM-form sub-account of an ME ID (anyone can; the contract checks x/did)"],
  bond: ["bond <deposit-id>[,<id>…] [--as owner]", "show ME Hub fixed deposits as your bond (empty list clears it)"],
  identity: ["identity <role|address>", "ME ID (direct or via its main account) and bond, as the contract sees them"],
  watch: ["watch [--owner o] [--webhook URL] [--from height] [--interval 6] [--once]", "follow payments as they happen; held ones ask for your approval (Slack-compatible webhook)"],
};

function usage(cmd) {
  if (cmd && HELP[cmd]) return `usage: leash ${HELP[cmd][0]}\n  ${HELP[cmd][1]}`;
  const w = Math.max(...Object.keys(HELP).map((k) => k.length));
  return `leash ${VERSION}: KYC-backed spending leashes for AI agents on ME Hub (${NET.chainId})\n\n` +
    Object.entries(HELP).map(([k, [, s]]) => `  ${k.padEnd(w)}  ${s}`).join("\n") +
    "\n\n  roles are key names in " + KEY_DIR + " (owner, agent, …) or any me1… address\n" +
    "  --as <role> signs as another role; --key-file <path> signs with that key file; --json for machine output\n" +
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

const CSV_HEAD = "id,status,created_utc,settled_utc,amount_mec,vendor,vendor_me_id,invoice,agent,owner,owner_me_id,approved_by,memo";
const csvCell = (v) => { const t = String(v ?? ""); return /[",\n]|^[=+\-@]/.test(t) ? `"${t.replace(/"/g, '""').replace(/^([=+\-@])/, "'$1")}"` : t; };
const iso = (s) => (s ? new Date(s * 1000).toISOString() : "");
const csvRow = (p) => [p.id, p.status, iso(p.created_at), iso(p.settled_at), fmt(p.amount).replace(" MEC", ""), p.vendor, p.vendor_did, p.invoice, p.agent, p.owner, p.owner_did, (p.approved_by ?? []).join(" "), p.memo].map(csvCell).join(",");
const print = (x) => console.log(o.json ? JSON.stringify(x, null, 2) : x);
const when = (s) => new Date(s * 1000).toISOString().slice(0, 16).replace("T", " ") + " UTC";
const payLine = (p) => `#${String(p.id).padEnd(5)} ${p.status.padEnd(8)} ${fmt(p.amount).padStart(14)}  ${p.vendor}  ${JSON.stringify(p.invoice)}  ${when(p.created_at)}`;

/** Prints a send outcome; exit code 0 only when it went through. */
function outcome(r) {
  if (o.json) { console.log(JSON.stringify({ ...r, events: undefined }, null, 2)); return r.status === "ok" ? 0 : r.status === "unknown" ? 3 : 1; }
  const a = r.attrs ?? {};
  switch (r.status) {
    case "ok":
      console.log(`ok   ${a.action ?? "done"}${a.status ? ` (${a.status})` : ""}${a.payment_id ? ` payment #${a.payment_id}` : ""}  ${r.link}`);
      const rest = Object.entries(a).filter(([k]) => k !== "action").map(([k, v]) => `${k}=${v}`).join(" ");
      if (rest) console.log("     " + rest);
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
// --key-file signs with that file instead of a role's key (e.g. the agent host, which holds only its own key).
// --fee-granter <addr|role> (or LEASH_FEE_GRANTER): fees come from that account's x/feegrant allowance.
const send = async (role, msg, funds) => outcome(await act(role, msg, funds, { file: o["key-file"], ...(o["fee-granter"] ? { granter: await addrOf(o["fee-granter"]) } : {}) }));

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
      console.log(`bond       ${bondLine(w.owner_bond)}`);
      const g = await feeGrant(agent, w.owner).catch(() => null);
      const own = fmt(await balance(agent));
      console.log(`gas        ${g ? `paid by the owner's fee grant: ${g.spend_limit ? fmt(g.spend_limit) : "no cap"} left${g.expiration ? `, until ${g.expiration.slice(0, 10)}` : ""}${g.allowed_messages ? ` (${g.allowed_messages.map((m) => m.split(".").pop()).join(", ")} only)` : ""}; the agent's own wallet holds ${own}` : `from the agent's own wallet: ${own}`}`);
      console.log(`can pay    ${w.can_pay_now ? "yes" : `NO: ${w.blocked_by}`}`);
      console.log(`limits     ≤ ${fmt(p.per_tx_limit)} per payment, ≤ ${fmt(p.daily_limit)} per 24h, approval above ${p.approve_above ? fmt(p.approve_above) : "never"}`);
      console.log(`24h spend  ${fmt(w.spent_24h)} (remaining ${fmt(w.remaining_24h)}); owner budget ${fmt(w.owner_balance)}; ${w.pending_count} held, ${w.approvals_needed} signature(s) release one`);
      console.log(`vendors    ${p.vendors.join(", ") || "(none)"}${p.any_verified_vendor ? " + any verified ME ID holder" : ""}\nexpires    ${when(p.expires_at)}${w.revoked ? "  REVOKED" : ""}`);
      return 0;
    } catch (e) {
      if (isNotFound(e)) { console.log(`${agent} is not a Leash ID agent`); return 1; }
      throw e;
    }
  },
  async owner() { print(await q({ owner: { owner: await addrOf(args[0] ?? "owner") } })); return 0; },
  async history() {
    if (o.csv) {
      const who = o.owner ? { payments_by_owner: { owner: await addrOf(o.owner) } } : { payments_by_agent: { agent: await addrOf(o.agent ?? "agent") } };
      const k = Object.keys(who)[0];
      console.log(CSV_HEAD);
      for (let before = null; ;) {
        const page = (await q({ [k]: { ...who[k], start_before: before, limit: 100 } })).payments;
        for (const p of page) console.log(csvRow(p));
        if (page.length < 100) return 0;
        before = page[page.length - 1].id;
      }
    }
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
// Payments as they happen, from the contract's `wasm-leash_payment` events. Held payments are the
// ones a person has to act on, so they're marked, and can be pushed to a Slack-compatible webhook.
cmds.watch = async () => {
  const r = await reader();
  const C = contract();
  const ownerF = o.owner ? await addrOf(o.owner) : null;
  const every = Number(o.interval ?? 6) * 1000;
  let from = o.from ? intArg(o.from, "--from") : Math.max(1, (await r.getHeight()) - 20);
  const seen = new Set();
  const hook = async (text, ev) => {
    if (!o.webhook) return;
    try {
      const res = await fetch(o.webhook, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ text, leash_payment: ev }) });
      if (!res.ok) console.error(`webhook: HTTP ${res.status}`);
    } catch (e) { console.error(`webhook: ${e.message}`); }
  };
  console.error(`watching ${C} on ${NET.chainId} from height ${from}${ownerF ? ` for owner ${ownerF}` : ""}${o.webhook ? " → webhook" : ""}`);
  for (;;) {
    let txs = [];
    try {
      const top = await r.getHeight();
      txs = top >= from ? await r.searchTx(`wasm-leash_payment._contract_address='${C}' AND tx.height>=${from}`) : [];
      from = Math.max(from, top - 1);
    } catch (e) { console.error(`(ME Hub unreachable: ${e.message.slice(0, 80)}; retrying)`); }
    for (const tx of txs.sort((a, b) => a.height - b.height)) {
      for (const [i, e] of tx.events.entries()) {
        if (e.type !== "wasm-leash_payment") continue;
        const a = Object.fromEntries(e.attributes.map((x) => [x.key, x.value]));
        const key = `${tx.hash}/${i}`;
        if (seen.has(key) || (ownerF && a.owner !== ownerF)) continue;
        seen.add(key);
        const held = a.kind === "created" && a.status === "pending";
        const text = `${held ? "NEEDS YOUR APPROVAL" : a.kind === "created" ? "paid" : a.kind}: #${a.payment_id} ${fmt(a.amount)} to ${a.vendor}` +
          `${a.vendor_did ? ` (ME ID ${a.vendor_did})` : ""} invoice ${a.invoice} by agent ${a.agent} [height ${tx.height}, tx ${tx.hash}]` +
          (held ? `  → leash approve ${a.payment_id}  or  leash reject ${a.payment_id}` : "");
        console.log(o.json ? JSON.stringify({ height: tx.height, tx: tx.hash, ...a }) : text);
        await hook(text, a);
      }
    }
    if (o.once) return 0;
    await new Promise((res) => setTimeout(res, every));
  }
};
cmds.approvers = async () => {
  if (!args.length && !o.threshold) { print(await q({ approvers: { owner: await addrOf(o.owner ?? "owner") } })); return 0; }
  const list = await Promise.all((args[0] ?? "").split(",").filter(Boolean).map(addrOf));
  return send(as("owner"), { set_approvers: { approvers: list, threshold: intArg(o.threshold ?? String(list.length + 1), "--threshold") } });
};
cmds["gas-grant"] = async () => {
  const agent = await addrOf(need(args[0], "agent"));
  const { addr } = await signer(as("owner"), o["key-file"]);
  const until = Math.floor(Date.now() / 1000) + intArg(o.days ?? "30", "--days") * 86400;
  const old = await feeGrant(agent, addr);
  const msgs = [...(old ? [revokeGasMsg(addr, agent)] : []), grantGasMsg(addr, agent, toUmec(need(o.limit, "--limit")), until)];
  return outcome(await sendTx(as("owner"), msgs, { file: o["key-file"], granter: null }));
};
cmds["gas-revoke"] = async () => {
  const agent = await addrOf(need(args[0], "agent"));
  const { addr } = await signer(as("owner"), o["key-file"]);
  return outcome(await sendTx(as("owner"), [revokeGasMsg(addr, agent)], { file: o["key-file"], granter: null }));
};
cmds["link-sub"] = async () => send(as("owner"), { register_sub_account: { sub: await addrOf(need(args[0], "sub-account")), main: await addrOf(need(args[1], "main account")) } });
cmds.bond = async () => send(as("owner"), { set_bond: { deposit_ids: (args[0] ?? "").split(",").filter(Boolean).map((x) => intArg(x, "deposit id")) } });
const bondLine = (b) => (b ? `${fmt(b.amount)} locked in ${b.deposits.length} ME Hub fixed deposit(s), first unlocks ${when(b.earliest_unlock)}` : "none");
cmds.identity = async () => {
  const r = await q({ identity: { address: await addrOf(need(args[0], "role or address")) } });
  if (o.json) return print(r), 0;
  const id = r.me_id;
  console.log(`address  ${r.address}\nME ID    ${id ? `${id.did} ${id.active ? "active" : "INACTIVE"} KYC ${id.kyc_level}${r.via_main ? ` (via its main account ${r.via_main})` : ""}` : "none"}\nbond     ${bondLine(r.bond)}`);
  return 0;
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
