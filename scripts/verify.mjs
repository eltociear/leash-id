#!/usr/bin/env node
// Is the contract on ME Hub the code in this repo? No keys needed; reads the chain over REST.
//   node scripts/verify.mjs            # deployment.json (testnet)
//   npm run build && node scripts/verify.mjs   # rebuild first, then compare
// Checks: contract -> code id -> on-chain wasm checksum == sha256(artifacts/leash_id.wasm),
// the contract admin, the configured KYC floor, and that the DID query answers from inside the contract.
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { NET, ROOT, deployment } from "../lib/chain.mjs";

const d = deployment();
const get = async (p) => {
  const r = await fetch(NET.rest + p);
  if (!r.ok) throw new Error(`${p}: HTTP ${r.status} ${await r.text()}`);
  return r.json();
};
const smart = async (msg) =>
  (await get(`/cosmwasm/wasm/v1/contract/${d.contract}/smart/${Buffer.from(JSON.stringify(msg)).toString("base64")}`)).data;

let ok = true;
const check = (cond, label, detail = "") => {
  ok &&= !!cond;
  console.log(`${cond ? "ok  " : "FAIL"}  ${label}${detail ? `  ${detail}` : ""}`);
};

const wasm = fs.readFileSync(path.join(ROOT, "artifacts/leash_id.wasm"));
const local = createHash("sha256").update(wasm).digest("hex");
const info = (await get(`/cosmwasm/wasm/v1/contract/${d.contract}`)).contract_info;
const code = (await get(`/cosmwasm/wasm/v1/code/${info.code_id}`)).code_info;
const onchain = code.data_hash.toLowerCase();

console.log(`chain     ${NET.chainId}  (${NET.rest})`);
console.log(`contract  ${d.contract}`);
check(String(info.code_id) === String(d.codeId), "code id matches deployment.json", `code ${info.code_id}`);
check(onchain === local, "on-chain wasm == artifacts/leash_id.wasm", `sha256 ${onchain}`);
console.log(`      admin ${info.admin || "(none)"}, creator ${info.creator}`);

const cfg = await smart({ config: {} });
check(cfg.min_kyc_level >= 1, "KYC floor set", `min_kyc_level ${cfg.min_kyc_level}, denom ${cfg.denom}`);

// The contract's own DID lookup (Stargate /metaearth.did.Query/Did), compared with x/did over REST.
const probe = process.argv[2] ?? info.creator;
if (probe) {
  const viaContract = await smart({ me_id: { address: probe } });
  const r = await fetch(`${NET.rest}/metaearth/did/did?address=${probe}`);
  const viaRest = r.ok ? (await r.json()).info ?? null : null;
  const same = viaContract && viaRest ? viaContract.did === viaRest.did : !viaContract && !viaRest;
  check(same, "contract's DID query agrees with x/did", `${probe} -> ${viaContract ? `ME ID ${viaContract.did}` : "no ME ID"}`);
}

console.log(ok ? "\nverified" : "\nMISMATCH");
process.exit(ok ? 0 : 1);
