// ME Hub plumbing: endpoints, keys, signing clients, contract helpers, amounts.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";
import { Secp256k1HdWallet, makeSignDoc, pubkeyToAddress, serializeSignDoc } from "@cosmjs/amino";
import { Secp256k1, Secp256k1Signature, sha256 } from "@cosmjs/crypto";
import { fromBase64, toBase64 } from "@cosmjs/encoding";
import { GasPrice } from "@cosmjs/stargate";
import { CosmWasmClient, SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const NET = {
  chainId: process.env.ME_CHAIN_ID ?? "mechain_400-1",
  rpc: process.env.ME_RPC ?? "https://beta-hub-26657.explorer-testnet.me",
  rest: process.env.ME_REST ?? "https://beta-hub-1317.explorer-testnet.me",
  explorer: process.env.ME_EXPLORER ?? "https://www.explorer-testnet.me",
  denom: process.env.ME_DENOM ?? "umec",
  prefix: "me",
  gasPrice: process.env.ME_GAS_PRICE ?? "0.1umec",
};

// ---------------- keys ----------------
//
// One file per role: <key dir>/<role>.mnemonic (mode 0600). Keeping roles in separate files is what
// lets an agent run with only its own key: see SECURITY in the README.
export const CONFIG_DIR = process.env.LEASH_HOME ?? path.join(os.homedir(), ".config/leash-id");
export const KEY_DIR = process.env.LEASH_KEY_DIR ?? path.join(CONFIG_DIR, "keys");
/** Pre-0.2 layout: every role in one keys.env. Still read, never written (`keygen --split` migrates it). */
export const LEGACY_KEYS_FILE = process.env.LEASH_KEYS ?? path.join(CONFIG_DIR, "keys.env");
// A local devnet keeps its own deployment file so it never overwrites the testnet one.
export const DEPLOY_FILE = path.resolve(ROOT, process.env.LEASH_DEPLOYMENT ?? "deployment.json");

const ROLE = /^[a-z0-9][a-z0-9_-]{0,31}$/;
export const keyFile = (role) => path.join(KEY_DIR, `${role}.mnemonic`);

/** Validates a mnemonic without ever echoing it. */
export function checkMnemonic(m, where) {
  const words = String(m ?? "").trim().split(/\s+/).filter(Boolean);
  if (![12, 15, 18, 21, 24].includes(words.length) || !words.every((w) => /^[a-z]+$/.test(w))) {
    throw new Error(`${where}: not a BIP-39 mnemonic (expected 12-24 lowercase words; contents not shown)`);
  }
  return words.join(" ");
}

function warnIfOpen(file) {
  try {
    if (process.platform !== "win32" && (fs.statSync(file).mode & 0o077)) {
      console.error(`warning: ${file} is readable by other users; run: chmod 600 ${file}`);
    }
  } catch {}
}

function readLegacy() {
  if (!fs.existsSync(LEGACY_KEYS_FILE)) return {};
  const out = {};
  fs.readFileSync(LEGACY_KEYS_FILE, "utf8").split("\n").forEach((line, i) => {
    const m = line.trim().match(/^(?:export\s+)?([A-Za-z0-9_]+)_MNEMONIC\s*=\s*(.*)$/);
    if (!m) return;
    const v = m[2].trim().replace(/^(["'])(.*)\1$/, "$2");
    out[m[1].toLowerCase().replace(/_/g, "-")] = { file: LEGACY_KEYS_FILE, line: i + 1, value: v };
  });
  return out;
}

/** Roles that have a key on this machine. */
export function roles() {
  const set = new Set(Object.keys(readLegacy()));
  if (fs.existsSync(KEY_DIR)) {
    for (const f of fs.readdirSync(KEY_DIR)) if (f.endsWith(".mnemonic")) set.add(f.slice(0, -9));
  }
  return [...set].sort();
}

/** Reads exactly one key: from an explicit file, from <key dir>/<role>.mnemonic, or the legacy keys.env. */
export function readKey(role, file) {
  if (file) {
    warnIfOpen(file);
    return checkMnemonic(fs.readFileSync(file, "utf8"), file);
  }
  if (!ROLE.test(role ?? "")) throw new Error(`bad role name "${role}" (use a-z, 0-9, - or _)`);
  // Serverless hosts keep secrets in env: LEASH_<ROLE>_MNEMONIC (e.g. LEASH_AGENT_MNEMONIC).
  const envKey = `LEASH_${role.toUpperCase().replace(/-/g, "_")}_MNEMONIC`;
  if (process.env[envKey]) return checkMnemonic(process.env[envKey], `env ${envKey}`);
  const f = keyFile(role);
  if (fs.existsSync(f)) {
    warnIfOpen(f);
    return checkMnemonic(fs.readFileSync(f, "utf8"), f);
  }
  const legacy = readLegacy()[role];
  if (legacy) {
    warnIfOpen(legacy.file);
    return checkMnemonic(legacy.value, `${legacy.file} line ${legacy.line}`);
  }
  throw new Error(`no key for "${role}": expected ${f} (create one with: leash keygen ${role})`);
}

const wallets = new Map();
export async function wallet(role, file) {
  const k = file ?? role;
  if (!wallets.has(k)) wallets.set(k, DirectSecp256k1HdWallet.fromMnemonic(readKey(role, file), { prefix: NET.prefix }));
  return wallets.get(k);
}

export async function address(role, file) {
  return (await (await wallet(role, file)).getAccounts())[0].address;
}

export const isAddress = (s) => typeof s === "string" && /^me1[02-9ac-hj-np-z]{38,58}$/.test(s);
/** A role name or an me1… address -> address. */
export const addrOf = async (x) => (isAddress(x) ? x : address(x));

// ---------------- clients ----------------

/** Retries only steps that cannot have sent anything (connecting, reading). Never wrap a broadcast. */
export async function retry(fn, tries = 4) {
  for (let i = 1; ; i++) {
    try {
      return await fn();
    } catch (e) {
      const transient = /fetch failed|ECONNRESET|ETIMEDOUT|ECONNREFUSED|socket hang up|network|502|503|504/i.test(String(e?.message ?? e));
      if (!transient || i >= tries) throw e;
      await new Promise((r) => setTimeout(r, 1500 * i));
    }
  }
}

const signers = new Map();
/** One cached signing client per key, so concurrent calls share it (see lib/tx.mjs for the send queue). */
export async function signer(role, file) {
  const k = file ?? role;
  if (!signers.has(k)) {
    signers.set(k, (async () => {
      const w = await wallet(role, file);
      const [{ address: addr }] = await w.getAccounts();
      const client = await retry(() => SigningCosmWasmClient.connectWithSigner(NET.rpc, w, {
        gasPrice: GasPrice.fromString(NET.gasPrice),
      }));
      return { client, addr };
    })().catch((e) => { signers.delete(k); throw e; }));
  }
  return signers.get(k);
}

let ro;
export async function reader() {
  ro ??= retry(() => CosmWasmClient.connect(NET.rpc)).catch((e) => { ro = undefined; throw e; });
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
  if (!c) throw new Error("no contract configured: set LEASH_CONTRACT, or deploy one with `leash deploy`");
  return c;
}

export async function q(msg) {
  const r = await reader();
  return retry(() => r.queryContractSmart(contract(), msg));
}

/** "not registered" style answers are real answers; anything else (network, 5xx) is not. */
export const isNotFound = (e) => /not registered|not found|no payment #/i.test(String(e?.message ?? e));

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

/** Contract metadata from wasmd: code id, creator, admin ("" = nobody can migrate it). */
export async function contractInfo(addr = contract()) {
  const r = await retry(() => fetch(`${NET.rest}/cosmwasm/wasm/v1/contract/${addr}`).then((r) => r.json()));
  if (!r.contract_info) throw new Error(`no contract at ${addr}: ${JSON.stringify(r).slice(0, 200)}`);
  return r.contract_info;
}

// ---------------- amounts ----------------

// ME Hub denom metadata: 1 MEC = 10^8 umec (exponent 8), not the usual 10^6.
export const DECIMALS = 8;
const ONE = 10n ** BigInt(DECIMALS);
const AMOUNT = new RegExp(`^\\d{1,20}(\\.\\d{1,${DECIMALS}})?$`);
/** "0.015" MEC -> "1500000" umec. Rejects anything that is not a plain decimal with <= 8 places. */
export function toUmec(mec) {
  const s = String(mec ?? "").trim();
  if (!AMOUNT.test(s)) throw new Error(`not a MEC amount: "${mec}" (use a plain decimal with at most ${DECIMALS} places, e.g. 0.015)`);
  const [w, f = ""] = s.split(".");
  const v = BigInt(w) * ONE + BigInt((f + "0".repeat(DECIMALS)).slice(0, DECIMALS));
  if (v === 0n) throw new Error("amount must be greater than 0");
  return v.toString();
}
export const fmt = (u) => {
  const v = BigInt(u);
  const frac = (v % ONE).toString().padStart(DECIMALS, "0").replace(/0+$/, "");
  return `${v / ONE}${frac ? "." + frac : ""} MEC`;
};
// A link anyone can open to check a tx: ME Hub's own REST answer for it (the testnet explorer's SPA has no stable
// tx URL). Override with ME_TX_URL, e.g. "https://explorer.example/tx/{hash}".
export const txUrl = (h) => (process.env.ME_TX_URL ? process.env.ME_TX_URL.replace("{hash}", h) : `${NET.rest}/cosmos/tx/v1beta1/txs/${h}`);

// ---------------- proofs (ADR-36 signArbitrary, the format Keplr uses) ----------------

const adr36 = (signer, data) => makeSignDoc(
  [{ type: "sign/MsgSignData", value: { signer, data: toBase64(new TextEncoder().encode(data)) } }],
  { gas: "0", amount: [] }, "", "", 0, 0);

/** Sign `data` with a role's key, off chain: { signer, pub_key, signature } (base64). */
export async function signProof(role, file, data) {
  const w = await Secp256k1HdWallet.fromMnemonic(readKey(role, file), { prefix: NET.prefix });
  const [{ address: signer }] = await w.getAccounts();
  const { signature } = await w.signAmino(signer, adr36(signer, data));
  return { signer, pub_key: signature.pub_key.value, signature: signature.signature };
}

/** True when `proof` is `data` signed by the key behind `proof.signer`. */
export async function verifyProof(proof, data) {
  try {
    if (pubkeyToAddress({ type: "tendermint/PubKeySecp256k1", value: proof.pub_key }, NET.prefix) !== proof.signer) return false;
    const hash = sha256(serializeSignDoc(adr36(proof.signer, data)));
    return await Secp256k1.verifySignature(Secp256k1Signature.fromFixedLength(fromBase64(proof.signature)), hash, fromBase64(proof.pub_key));
  } catch {
    return false;
  }
}
