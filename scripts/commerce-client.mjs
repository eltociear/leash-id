#!/usr/bin/env node
// Calls the MCP server's pay_for_api tool once, as an agent framework would.  node scripts/commerce-client.mjs <url> <max_mec>
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
const [url, max] = process.argv.slice(2);
const c = new Client({ name: "commerce-demo", version: "1" });
await c.connect(new StdioClientTransport({ command: "node", args: [new URL("../mcp/server.mjs", import.meta.url).pathname], env: { ...process.env, LEASH_DEMO_SHARED_KEYS: "1", LEASH_ALLOW_LOCAL_URLS: "1", LEASH_LOG_DIR: process.env.LEASH_LOG_DIR ?? "runs" }, stderr: "ignore" }));
const r = await c.callTool({ name: "pay_for_api", arguments: { url, max_mec: max } });
console.log(`[${r.structuredContent?.status}${r.isError ? ", isError" : ""}] ${r.content[0].text}`);
if (r.structuredContent?.tx) console.log(`tx=${r.structuredContent.tx} invoice=${r.structuredContent.invoice ?? ""}`);
await c.close();
