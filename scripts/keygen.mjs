// Creates testnet-only demo keys in ~/.config/leash-id/keys.env (never in the repo).
import { DirectSecp256k1HdWallet } from "@cosmjs/proto-signing";
import fs from "node:fs"; import os from "node:os"; import path from "node:path";
const dir = path.join(os.homedir(), ".config/leash-id"); const file = path.join(dir, "keys.env");
fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
const have = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "";
let out = have;
for (const role of process.argv.slice(2).length ? process.argv.slice(2) : ["owner", "agent", "vendor", "rogue"]) {
  const k = `${role.toUpperCase()}_MNEMONIC`;
  if (have.includes(k + "=")) continue;
  const w = await DirectSecp256k1HdWallet.generate(24, { prefix: "me" });
  out += `${k}="${w.mnemonic}"\n`;
}
fs.writeFileSync(file, out, { mode: 0o600 });
for (const line of out.trim().split("\n")) {
  const [k, v] = line.split(/=(.*)/s); const w = await DirectSecp256k1HdWallet.fromMnemonic(JSON.parse(v), { prefix: "me" });
  console.log(k.replace("_MNEMONIC", "").toLowerCase().padEnd(7), (await w.getAccounts())[0].address);
}
