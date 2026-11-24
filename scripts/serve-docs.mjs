#!/usr/bin/env node
// Serves docs/ for local testing and forwards /rpc → the local node RPC (:36657) and everything else → REST (:1317),
// so the pages work against a devnet without CORS changes.   node scripts/serve-docs.mjs [port]
import http from "node:http"; import { readFileSync, existsSync } from "node:fs"; import path from "node:path";
const dir = new URL("../docs", import.meta.url).pathname;
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png" };
http.createServer(async (req, res) => {
  const p0 = req.url.split("?")[0];
  const file = path.join(dir, p0 === "/" ? "index.html" : p0);
  if (!p0.startsWith("/rpc") && file.startsWith(dir) && existsSync(file) && !file.endsWith("/")) {
    res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" }); return res.end(readFileSync(file));
  }
  const body = await new Promise((r) => { const c = []; req.on("data", (d) => c.push(d)); req.on("end", () => r(Buffer.concat(c))); });
  const target = p0.startsWith("/rpc") ? "http://localhost:36657" + req.url.slice(4) : "http://localhost:1317" + req.url;
  const r = await fetch(target, { method: req.method, body: ["GET", "HEAD"].includes(req.method) ? undefined : body, headers: { "content-type": req.headers["content-type"] ?? "application/json" } }).catch(() => null);
  if (!r) { res.writeHead(502); return res.end(); }
  res.writeHead(r.status, { "content-type": r.headers.get("content-type") ?? "application/json" }); res.end(Buffer.from(await r.arrayBuffer()));
}).listen(Number(process.argv[2] ?? 18766), () => console.log(`docs on http://localhost:${process.argv[2] ?? 18766}`));
