#!/usr/bin/env bash
# Demo of awkrs's two performance dimensions:
#   • parallel record processing (-j N) — only valid for parallel-safe programs
#   • Cranelift JIT (default) vs bytecode VM (AWKRS_JIT=0)
# Generates a 200 K-line file and runs two workloads:
#   • Phase A: `{ print toupper($0) }` — parallel-safe stateless transform
#   • Phase B: `{ s += $1 } END { print s }` — cross-record accumulator (sequential)
# Uses /usr/bin/time -p so this works without hyperfine. For statistically
# rigorous A/B numbers, use scripts/benchmark-readme-jit-vs-vm.sh and
# scripts/benchmark-vs-awk.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT" || exit 1

AWKRS="$ROOT/target/debug/awkrs"
if [[ ! -x "$AWKRS" ]]; then
  echo "Building awkrs (debug)..."
  command cargo build -q
fi

LINES="${LINES:-200000}"
TMP="$(command mktemp "${TMPDIR:-/tmp}/awkrs-demo.XXXXXX")"
OUT="$(command mktemp "${TMPDIR:-/tmp}/awkrs-demo-out.XXXXXX")"
trap 'command rm -f "$TMP" "$OUT"' EXIT

echo "Generating $LINES × 5-field rows → $TMP"
"$AWKRS" -v n="$LINES" 'BEGIN { for (i = 1; i <= n; i++) print "rec", i, i+1, i*2, i*3 }' >"$TMP"

THREADS="$(command sysctl -n hw.logicalcpu 2>/dev/null || command nproc 2>/dev/null || echo 4)"

# /usr/bin/time -p prints real/user/sys on stderr. We redirect stdout to /dev/null
# for the transform phase (output is N lines; we only care about timing here).
time_run_silent() {
  local label="$1"
  shift
  command printf '\n── %s ──\n' "$label"
  command /usr/bin/time -p "$@" >"$OUT" 2> >(command sed 's/^/  time: /' >&2)
}
time_run_show() {
  local label="$1"
  shift
  command printf '\n── %s ──\n' "$label"
  command /usr/bin/time -p "$@" 2> >(command sed 's/^/  time: /' >&2) | command sed 's/^/  out:  /'
}

XFORM='{ print toupper($0) }'
AGG='{ s += $2 } END { print "sum_col2 =", s }'

command printf '\n%s=== Phase A: parallel-safe transform (toupper) ===%s\n' "" ""

time_run_silent "[A1] -j 1 + JIT"                env -u AWKRS_JIT "$AWKRS" -j 1            "$XFORM" "$TMP"
time_run_silent "[A2] -j 1 + bytecode"           env AWKRS_JIT=0   "$AWKRS" -j 1            "$XFORM" "$TMP"
time_run_silent "[A3] -j $THREADS + JIT"         env -u AWKRS_JIT "$AWKRS" -j "$THREADS"   "$XFORM" "$TMP"
time_run_silent "[A4] -j $THREADS + bytecode"    env AWKRS_JIT=0   "$AWKRS" -j "$THREADS"   "$XFORM" "$TMP"

command printf '\n%s=== Phase B: cross-record aggregate (sequential) ===%s\n' "" ""

time_run_show "[B1] -j 1 + JIT"                  env -u AWKRS_JIT "$AWKRS" -j 1 "$AGG" "$TMP"
time_run_show "[B2] -j 1 + bytecode"             env AWKRS_JIT=0   "$AWKRS" -j 1 "$AGG" "$TMP"

command printf '\nDone. Each phase ran on the same input. Numbers vary across machines.\n'
command printf 'For statistically rigorous A/B, run scripts/benchmark-readme-jit-vs-vm.sh\n'
command printf '(hyperfine; --warmup 2 --min-runs 8).\n'
