#!/usr/bin/env bash
# Regenerate docs/awkrs-help.png from `awkrs -h` using termshot.
# Requires: termshot (e.g. brew install termshot), Python 3, cargo build output.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build -q
mkdir -p assets
RAW="$ROOT/assets/help.raw"
BIN="$ROOT/target/debug/awkrs"
python3 "$ROOT/scripts/capture-help-pty.py" "$BIN" >"$RAW"
termshot --raw-read "$RAW" -C 100 -m 32 -p 20 -f "$ROOT/assets/awkrs-help.png"
command rm -f "$RAW"
echo "Wrote $ROOT/assets/awkrs-help.png"
