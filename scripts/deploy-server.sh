#!/usr/bin/env bash
# Deploy gyat-server to your remote box and verify it works.
#
#   ./scripts/deploy-server.sh                 # uses ~/.gyatconfig.toml
#   ./scripts/deploy-server.sh water@100.81.91.113
#
# Transport is one-shot commands over ssh (like git) — there is no daemon to
# start. After this you only ever run `gyat push` / `gyat pull` / `gyat clone`.
set -euo pipefail

TARGET="${1:-}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/gyat-server"

if [ -z "$TARGET" ]; then
  CFG="${GYAT_CONFIG:-$HOME/.gyatconfig.toml}"
  if [ ! -f "$CFG" ]; then
    echo "no target given and no $CFG — run 'gyat setup' or pass user@host" >&2
    exit 1
  fi
  TARGET="$(awk -F'"' '/^host[[:space:]]*=/ {h=$2} /^user[[:space:]]*=/ {u=$2} END {if (h=="") print ""; else print (u=="" ? h : u "@" h)}' "$CFG")"
  if [ -z "$TARGET" ]; then
    echo "could not read host/user from $CFG" >&2
    exit 1
  fi
  echo "==> target from $CFG: $TARGET"
fi

[ -f "$BIN" ] || { echo "building release binary..."; cargo build --release -p gyat-server --manifest-path "$ROOT/Cargo.toml"; }

echo "==> uploading gyat-server ($(du -h "$BIN" | cut -f1)) to $TARGET"
# ~/.local/bin is on the non-interactive PATH on Arch/fish + most setups
scp -q "$BIN" "$TARGET:~/.local/bin/gyat-server.tmp"
ssh -o BatchMode=yes "$TARGET" 'mkdir -p ~/.local/bin ~/gyat-server-data && mv ~/.local/bin/gyat-server.tmp ~/.local/bin/gyat-server && chmod +x ~/.local/bin/gyat-server'

echo "==> verifying"
ssh -o BatchMode=yes "$TARGET" 'gyat-server --version && gyat-server list ~/gyat-server-data >/dev/null 2>&1; echo "  server responds, data dir ready"'

echo
echo "done. you never need a URL again:"
echo "  gyat push / gyat pull / gyat clone <name> / gyat list / gyat list <repo>"
