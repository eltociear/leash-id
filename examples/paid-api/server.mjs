#!/usr/bin/env node
// A paid API that sells to AI agents, but only to agents with a verified human behind them (lib/vendor.mjs).
// It serves real data: ME Hub's latest block and this contract's payment count.
//   LEASH_PAY_TO=me1… [PRICE_MEC=0.001] [MIN_KYC=2] [PORT=18790] node examples/paid-api/server.mjs
import http from "node:http";
import { NET, address, reader, toUmec } from "../../lib/chain.mjs";
import { paywall } from "../../lib/vendor.mjs";

const payTo = process.env.LEASH_PAY_TO ?? (await address(process.env.LEASH_PAY_TO_ROLE ?? "shop"));
const priceUmec = toUmec(process.env.PRICE_MEC ?? "0.001");
const port = Number(process.env.PORT ?? 18790);

const handler = paywall({ payTo, priceUmec, minKyc: Number(process.env.MIN_KYC ?? 2), describe: "GET /chain-stats" }, async (req, res, paid) => {
  const r = await reader();
  const block = await r.getBlock();
  res.writeHead(200, { "content-type": "application/json" });
  res.end(JSON.stringify({
    chain_id: NET.chainId,
    height: block.header.height,
    time: block.header.time,
    txs_in_block: block.txs.length,
    receipt: { paid_by_agent: paid.agent, owner: paid.owner, owner_me_id: paid.owner_did, payment_id: paid.payment_id, tx: paid.tx },
  }, null, 2));
});

http.createServer((req, res) => (req.url.split("?")[0] === "/chain-stats" ? handler(req, res) : (res.writeHead(404), res.end()))).listen(port, () =>
  console.error(`paid API on http://localhost:${port}/chain-stats: ${process.env.PRICE_MEC ?? "0.001"} MEC per call to ${payTo}`));
