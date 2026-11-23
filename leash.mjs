#!/usr/bin/env node
// Leash ID CLI — KYC-backed spending leashes for AI agents on ME Network.
import fs from "node:fs";
import path from "node:path";
import {
  NET, ROOT, address, balance, contract, deployment, exec, fmt, meIdRest, q, readKeys,
  saveDeployment, signer, txUrl,
} from "./lib/chain.mjs";

const [cmd, ...args] = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : def;
};
const flag = (name) => args.includes(`--${name}`);
/** "1.5" MEC -> "1500000" umec */
const umec = (mec) => {
  const [w, f = ""] = String(mec).split(".");
  return (BigInt(w) * 1_000_000n + BigInt((f + "000000").slice(0, 6))).toString();
};
const role = () => opt("as", undefined);
const addrOf = async (x) => (x?.startsWith("me1") ? x : address(x));
const out = (r) => {
  if (r?.transactionHash) {
    console.log(`ok  tx ${r.transactionHash}  gas ${r.gasUsed}  ${txUrl(r.transactionHash)}`);
    const ev = r.events?.filter((e) => e.type === "wasm").flatMap((e) => e.attributes) ?? [];
    if (ev.length) console.log("   ", ev.filter((a) => a.key !== "_contract_address").map((a) => `${a.key}=${a.value}`).join(" "));
  } else console.log(JSON.stringify(r, null, 2));
};

const policy = async () => ({
  label: opt("label", "ap-clerk"),
  per_tx_limit: umec(opt("per-tx", "2")),
  daily_limit: umec(opt("daily", "5")),
  approve_above: opt("approve-above") ? umec(opt("approve-above")) : null,
  vendors: await Promise.all((opt("vendors", "vendor")).split(",").filter(Boolean).map(addrOf)),
  any_verified_vendor: flag("any-verified-vendor"),
  expires_at: Math.floor(Date.now() / 1000) + Number(opt("days", "30")) * 86400,
});

const cmds = {
  async addrs() {
    for (const r of Object.keys(readKeys())) {
      const a = await address(r);
      const id = await meIdRest(a).catch(() => null);
      console.log(r.padEnd(7), a, fmt(await balance(a)).padStart(14), id ? `ME ID ${id.did} ${id.status} ${id.kycLevel}` : "no ME ID");
    }
  },
  async meid() {
    const a = await addrOf(args[0] ?? "owner");
    console.log(JSON.stringify({ rest: await meIdRest(a), contract: deployment().contract ? await q({ me_id: { address: a } }) : "n/a" }, null, 2));
  },
  async deploy() {
    const wasm = fs.readFileSync(path.join(ROOT, "artifacts/leash_id.wasm"));
    const { client, addr } = await signer(opt("as", "owner"));
    const up = await client.upload(addr, wasm, "auto", "leash-id");
    console.log("stored code", up.codeId, up.transactionHash);
    const init = await client.instantiate(
      addr, up.codeId,
      { min_kyc_level: Number(opt("min-kyc", "2")), denom: NET.denom },
      "leash-id", "auto", { admin: addr },
    );
    console.log("contract", init.contractAddress, init.transactionHash);
    saveDeployment({
      ...deployment(), chainId: NET.chainId, codeId: up.codeId, contract: init.contractAddress,
      storeTx: up.transactionHash, instantiateTx: init.transactionHash,
      wasmSha256: (await import("node:crypto")).createHash("sha256").update(wasm).digest("hex"),
      deployedAt: new Date().toISOString(),
    });
  },
  async deposit() { out(await exec(role() ?? "owner", { deposit: {} }, [{ denom: NET.denom, amount: umec(args[0]) }])); },
  async withdraw() { out(await exec(role() ?? "owner", { withdraw: { amount: umec(args[0]) } })); },
  async register() {
    out(await exec(role() ?? "owner", { register_agent: { agent: await addrOf(args[0] ?? "agent"), policy: await policy() } }));
  },
  async policy() {
    out(await exec(role() ?? "owner", { update_policy: { agent: await addrOf(args[0] ?? "agent"), policy: await policy() } }));
  },
  async revoke() { out(await exec(role() ?? "owner", { revoke_agent: { agent: await addrOf(args[0] ?? "agent") } })); },
  async pay() {
    const [vendor, amount, invoice] = args;
    out(await exec(role() ?? "agent", {
      pay: { vendor: await addrOf(vendor), amount: umec(amount), invoice, memo: opt("memo", null) },
    }));
  },
  async batch() {
    // batch vendor:amount:invoice ...
    const payments = await Promise.all(args.filter((a) => a.includes(":")).map(async (s) => {
      const [v, a, i] = s.split(":");
      return { vendor: await addrOf(v), amount: umec(a), invoice: i };
    }));
    out(await exec(role() ?? "agent", { pay_batch: { payments } }));
  },
  async approve() { out(await exec(role() ?? "owner", { approve: { id: Number(args[0]) } })); },
  async reject() { out(await exec(role() ?? "owner", { reject: { id: Number(args[0]) } })); },
  async whois() { out(await q({ whois: { agent: await addrOf(args[0] ?? "agent") } })); },
  async owner() { out(await q({ owner: { owner: await addrOf(args[0] ?? "owner") } })); },
  async check() {
    const [vendor, amount, invoice] = args;
    out(await q({ check_pay: { agent: await addrOf(opt("agent", "agent")), vendor: await addrOf(vendor), amount: umec(amount), invoice } }));
  },
  async payments() { out(await q({ payments: { limit: 100 } })); },
  async info() { console.log({ ...NET, contract: contract(), ...deployment() }); },
};

if (!cmds[cmd]) {
  console.log(`usage: node leash.mjs <${Object.keys(cmds).join("|")}> …
  roles: owner agent vendor rogue (keys in ~/.config/leash-id/keys.env), or any me1… address
  amounts are in MEC (e.g. 1.5)`);
  process.exit(cmd ? 1 : 0);
}
cmds[cmd]().catch((e) => { console.error("ERR", e.message ?? e); process.exit(1); });
