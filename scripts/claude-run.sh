#!/usr/bin/env bash
# Claude Code as the AP agent, over MCP only (no built-in tools, no user settings/plugins/hooks).
# Uses mcp/demo.mcp.json: owner-only honeypot tools on, refusals recorded on chain, log in runs/.
#   scripts/claude-run.sh [prefix]   -> runs/<prefix>-<ts>.jsonl (default prefix: claude)
# The transcript keeps only the model's turns, the tool results and the final report, with $HOME masked.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p runs
OUT="runs/${1:-claude}-$(date -u +%Y-%m-%dT%H-%M-%SZ).jsonl"
claude -p "$(cat mcp/ap-clerk.prompt.txt)" \
  --mcp-config mcp/demo.mcp.json --strict-mcp-config --setting-sources project --no-session-persistence \
  --tools "" --allowedTools "mcp__leash-id" --max-turns 40 --output-format stream-json --verbose \
  | jq -c 'select(.type == "assistant" or .type == "user" or .type == "result")' \
  | sed "s#$HOME#~#g" > "$OUT"
jq -r 'select(.type=="result") | .result' "$OUT"
echo "-> $OUT"
