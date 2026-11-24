// Writes devnet/keys.txt: "<role> <address> <pubkey json>" for the Leash ID roles (public data only).
import { writeFileSync } from "node:fs";
import { wallet } from "../lib/chain.mjs";
const rows = [];
for (const r of ["owner", "agent", "vendor", "rogue", "lowkyc", "shop", "merchant"]) {
  const [a] = await (await wallet(r)).getAccounts();
  rows.push(`${r} ${a.address} ${JSON.stringify({ "@type": "/cosmos.crypto.secp256k1.PubKey", key: Buffer.from(a.pubkey).toString("base64") })}`);
}
writeFileSync(new URL("keys.txt", import.meta.url), rows.join("\n") + "\n");
console.log(rows.join("\n"));
