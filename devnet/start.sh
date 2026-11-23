#!/usr/bin/env bash
# Runs the local ME Hub node in the foreground (RPC :36657, REST :1317). Logs to node.log.
D="$(cd "$(dirname "$0")" && pwd)"
MED="${ME_HUB_DIR:-$HOME/workspace/harbor/vendor/me-hub}/build/med"
cd "$D" && HOME="$D/home" exec "$MED" start > "$D/node.log" 2>&1
