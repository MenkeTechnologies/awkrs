#!/usr/bin/env bash
# Regenerate assets/awkrs-help.png from `awkrs -h` using termshot (full-width color).
# Requires: termshot (e.g. brew install termshot), Python 3, cargo build output.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build -q
mkdir -p assets
RAW="$ROOT/assets/help.raw"
BIN="$ROOT/target/debug/awkrs"
python3 "$ROOT/scripts/capture-help-pty.py" "$BIN" >"$RAW"
# Wider than longest help line so output is not wrapped; termshot renders full color from PTY capture.
termshot --raw-read "$RAW" -C 256 -m 32 -p 20 -f "$ROOT/assets/awkrs-help.png"
command rm -f "$RAW"
echo "Wrote $ROOT/assets/awkrs-help.png"
