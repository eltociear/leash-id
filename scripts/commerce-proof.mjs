#!/usr/bin/env node
// Prints an X-Leash-Proof header value: <role>'s ADR-36 signature over the paywall proof text.
//   node scripts/commerce-proof.mjs <role> <tx> <invoice>
import { signProof } from "../lib/chain.mjs";
import { proofText } from "../lib/vendor.mjs";
const [role, tx, invoice] = process.argv.slice(2);
const p = await signProof(role, undefined, proofText(tx, invoice));
console.log(Buffer.from(JSON.stringify({ pub_key: p.pub_key, signature: p.signature })).toString("base64"));
