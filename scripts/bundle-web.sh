#!/usr/bin/env bash
# Rebuilds docs/vendor/cosmjs.js from the locked npm dependencies. The pages load only this file, never a CDN;
# CI rebuilds it and fails if it differs from the committed one.
set -euo pipefail
cd "$(dirname "$0")/.."
npx esbuild web/cosmjs-entry.mjs --bundle --format=esm --platform=browser --minify --legal-comments=eof --outfile=docs/vendor/cosmjs.js --log-level=warning
shasum -a 256 docs/vendor/cosmjs.js
