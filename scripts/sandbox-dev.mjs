#!/usr/bin/env node
// Runs the red-team sandbox locally (the same handlers Vercel serves from api/). Point it at a devnet with
// `source devnet/env.sh`; it signs with the local `agent` key (or LEASH_AGENT_MNEMONIC).
//   node scripts/sandbox-dev.mjs [port]      -> http://localhost:18780/
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const handlers = Object.fromEntries(await Promise.all(["state", "attack", "agent"].map(async (n) => [n, (await import(`../api/${n}.mjs`)).default])));
const types = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript" };
const port = Number(process.argv[2] ?? 18780);
http.createServer(async (req, res) => {
  const p = req.url.split("?")[0];
  const api = p.match(/^\/api\/(\w+)$/);
  if (api && handlers[api[1]]) {
    try { return await handlers[api[1]](req, res); } catch (e) { console.error(e); res.writeHead(500); return res.end(); }
  }
  const file = path.join(root, p === "/" ? "sandbox/index.html" : p.endsWith("/") ? p + "index.html" : p);
  if (!file.startsWith(path.join(root, "sandbox")) && !file.startsWith(path.join(root, "docs"))) { res.writeHead(404); return res.end(); }
  if (!fs.existsSync(file)) { res.writeHead(404); return res.end(); }
  res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
  res.end(fs.readFileSync(file));
}).listen(port, () => console.error(`sandbox on http://localhost:${port}/`));
