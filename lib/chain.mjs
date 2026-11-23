// ME Hub testnet plumbing: endpoints, keys, signing clients, contract helpers.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";
import { GasPrice } from "@cosmjs/stargate";
import { CosmWasmClient, SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const NET = {
  chainId: process.env.ME_CHAIN_ID ?? "mechain_400-1",
  rpc: process.env.ME_RPC ?? "https://beta-hub-26657.explorer-testnet.me",
  rest: process.env.ME_REST ?? "https://beta-hub-1317.explorer-testnet.me",
  evm: "https://beta-hub-8545.explorer-testnet.me",
  explorer: process.env.ME_EXPLORER ?? "https://www.explorer-testnet.me",
  denom: "umec",
  prefix: "me",
  gasPrice: process.env.ME_GAS_PRICE ?? "0.1umec",
};

export const KEYS_FILE = process.env.LEASH_KEYS ?? path.join(os.homedir(), ".config/leash-id/keys.env");
// A local devnet keeps its own deployment file so it never overwrites the testnet one.
export const DEPLOY_FILE = path.join(ROOT, process.env.LEASH_DEPLOYMENT ?? "deployment.json");

export function readKeys() {
  if (!fs.existsSync(KEYS_FILE)) return {};
  const out = {};
  for (const line of fs.readFileSync(KEYS_FILE, "utf8").split("\n")) {
    const m = line.match(/^([A-Z_]+)_MNEMONIC=(.*)$/);
    if (m) out[m[1].toLowerCase()] = JSON.parse(m[2]);
  }
  return out;
}

export async function wallet(role) {
  const k = readKeys()[role];
  if (!k) throw new Error(`no key for role "${role}" in ${KEYS_FILE} (run: node scripts/keygen.mjs ${role})`);
  return DirectSecp256k1HdWallet.fromMnemonic(k, { prefix: NET.prefix });
}

export async function address(role) {
  return (await (await wallet(role)).getAccounts())[0].address;
}

/** Retries only steps that cannot have sent anything (connecting, reading). Never wrap a broadcast. */
export async function retry(fn, tries = 4) {
  for (let i = 1; ; i++) {
    try {
      return await fn();
    } catch (e) {
      const transient = /fetch failed|ECONNRESET|ETIMEDOUT|socket hang up|network/i.test(String(e?.message ?? e));
      if (!transient || i >= tries) throw e;
      await new Promise((r) => setTimeout(r, 1500 * i));
    }
  }
}

export async function signer(role) {
  const w = await wallet(role);
  const [{ address: addr }] = await w.getAccounts();
  const client = await retry(() => SigningCosmWasmClient.connectWithSigner(NET.rpc, w, {
    gasPrice: GasPrice.fromString(NET.gasPrice),
  }));
  // Warm the account lookup too, so a flaky first read does not surface as a failed send.
  await retry(() => client.getSequence(addr)).catch(() => {});
  return { client, addr };
}

let ro;
export async function reader() {
  ro ??= await retry(() => CosmWasmClient.connect(NET.rpc));
  return ro;
}

export function deployment() {
  return fs.existsSync(DEPLOY_FILE) ? JSON.parse(fs.readFileSync(DEPLOY_FILE, "utf8")) : {};
}

export function saveDeployment(d) {
  fs.writeFileSync(DEPLOY_FILE, JSON.stringify(d, null, 2) + "\n");
}

export function contract() {
  const c = process.env.LEASH_CONTRACT ?? deployment().contract;
  if (!c) throw new Error("no contract yet: run `node leash.mjs deploy`");
  return c;
}

export async function q(msg) {
  const r = await reader();
  return retry(() => r.queryContractSmart(contract(), msg));
}

export async function exec(role, msg, funds = [], memo = "") {
  const { client, addr } = await signer(role);
  return client.execute(addr, contract(), msg, "auto", memo, funds);
}

export async function balance(addr) {
  const r = await retry(() => fetch(`${NET.rest}/cosmos/bank/v1beta1/balances/${addr}`).then((r) => r.json()));
  const c = (r.balances ?? []).find((b) => b.denom === NET.denom);
  return BigInt(c?.amount ?? 0);
}

/** ME ID straight from the DID module REST API (what the contract sees via Stargate). */
export async function meIdRest(addr) {
  const r = await retry(() => fetch(`${NET.rest}/metaearth/did/did?address=${addr}`).then((r) => r.json()));
  return r.info?.did ? r.info : null;
}

export const fmt = (u) => `${(Number(u) / 1e6).toFixed(6).replace(/\.?0+$/, "")} MEC`;
export const txUrl = (h) => `${NET.explorer}/tx/${h}`;
