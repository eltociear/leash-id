#!/usr/bin/env node
// Vercel build step for the red-team sandbox: bundle each function in api/ into one ES module, in place.
// Vercel's Node loader can't require() an ES module, and @cosmjs/encoding (CommonJS) requires @scure/base 2.x
// (ES only). Bundled, there's no require() of an ES module left. Local runs and CI use the sources as they are.
import { build } from "esbuild";
import fs from "node:fs";
// the static page, and the shared CSS it loads from /docs/style.css
fs.rmSync("public", { recursive: true, force: true });
fs.mkdirSync("public/docs", { recursive: true });
fs.cpSync("sandbox", "public/sandbox", { recursive: true });
fs.copyFileSync("docs/style.css", "public/docs/style.css");
for (const f of ["api/state.mjs", "api/attack.mjs", "api/agent.mjs"]) {
  await build({
    entryPoints: [f], outfile: f, allowOverwrite: true, bundle: true, platform: "node", format: "esm", target: "node22",
    // CommonJS deps inside an ES bundle still call require() for Node built-ins
    banner: { js: 'import { createRequire as __cr } from "node:module"; const require = __cr(import.meta.url);' },
    logLevel: "warning",
  });
  console.log("bundled", f);
}
