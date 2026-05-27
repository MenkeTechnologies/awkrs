#!/usr/bin/env bash
# Quickstart demo — runs a tour of awkrs features against the local debug build.
# Each section prints the command, then its output, so you can paste the same
# snippets into your shell. Builds awkrs (debug) if the binary is missing.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT" || exit 1

AWKRS="$ROOT/target/debug/awkrs"
if [[ ! -x "$AWKRS" ]]; then
  echo "Building awkrs (debug)..."
  command cargo build -q
fi

# ANSI codes are intentional — this is a demo, not log output. Turn off via NO_COLOR=1.
if [[ -n "${NO_COLOR:-}" ]] || [[ ! -t 1 ]]; then
  C_HDR=""; C_CMD=""; C_OUT=""; C_OFF=""
else
  C_HDR=$'\033[1;36m'; C_CMD=$'\033[1;33m'; C_OUT=$'\033[0;37m'; C_OFF=$'\033[0m'
fi

section() {
  command printf '\n%s━━ %s %s\n' "$C_HDR" "$1" "$C_OFF"
}
run() {
  command printf '%s$ %s%s\n' "$C_CMD" "$1" "$C_OFF"
  command printf '%s' "$C_OUT"
  eval "$1"
  command printf '%s' "$C_OFF"
}

section "[1] Hello, awk"
run "$AWKRS 'BEGIN { print \"hello from awkrs\" }'"

section "[2] Print second field, default whitespace FS"
run "command printf 'alpha bravo charlie\ndelta echo foxtrot\n' | $AWKRS '{ print \$2 }'"

section "[3] Sum first column with BEGIN/END"
run "command seq 1 100 | $AWKRS 'BEGIN { print \"summing\" } { s += \$1 } END { print \"sum =\", s }'"

section "[4] Word frequency with associative arrays + sorted iteration"
run "command printf 'the quick brown fox\nthe lazy dog\nthe brown dog\n' | $AWKRS '
{ for (i = 1; i <= NF; i++) c[\$i]++ }
END {
  PROCINFO[\"sorted_in\"] = \"@val_num_desc\"
  for (w in c) printf \"%-8s %d\n\", w, c[w]
}'"

section "[5] CSV mode (-k) — quoted commas stay in one field"
run "command printf 'name,address,age\nAlice,\"1 Main St, Apt 4\",30\nBob,\"42 Elm Rd\",25\n' | $AWKRS -k 'NR > 1 { print \$1 \" lives at \" \$2 }'"

section "[6] FIELDWIDTHS — fixed-width record splitting"
run "command printf '01234567890123456789\nABCDEFGHIJKLMNOPQRST\n' | $AWKRS 'BEGIN { FIELDWIDTHS = \"5 5 5 5\" } { print \$1, \$2, \$3, \$4 }'"

section "[7] FPAT — fields are what matches the pattern"
run "command printf 'a, \"hello world\", 42\n' | $AWKRS 'BEGIN { FPAT = \"([^,]+)|(\\\"[^\\\"]+\\\")\" } { for (i = 1; i <= NF; i++) print i, \$i }'"

section "[8] Multi-character RS regex with RT capture"
run "command printf 'aXXbYYYcXd' | $AWKRS 'BEGIN { RS = \"X+|Y+\" } { print NR, \"[\" \$0 \"]\", \"sep=\" RT }'"

section "[9] gensub with backreferences (g/Nth/single)"
run "$AWKRS 'BEGIN {
  s = \"alice@example.com bob@test.org\"
  print gensub(/([a-z]+)@([a-z.]+)/, \"\\\\2 [at] \\\\1\", \"g\", s)
}'"

section "[10] match() three-arg form populates capture subarray"
run "command printf 'order #1234 ships\norder #99 ships\n' | $AWKRS 'match(\$0, /#([0-9]+)/, m) { print \"id =\", m[1] }'"

section "[11] User functions: recursion + local via extra param"
run "$AWKRS 'function fact(n,   r, j) { r = 1; for (j = 2; j <= n; j++) r *= j; return r }
BEGIN { for (i = 1; i <= 8; i++) printf \"%d! = %d\n\", i, fact(i) }'"

section "[12] Pipe into a long-lived sort process"
run "$AWKRS 'BEGIN {
  print \"delta\" | \"sort\"
  print \"alpha\" | \"sort\"
  print \"charlie\" | \"sort\"
  print \"bravo\" | \"sort\"
}'"

section "[13] getline from a command pipeline"
run "$AWKRS 'BEGIN {
  cmd = \"printf \\\"one\\ntwo\\nthree\\n\\\"\"
  while ((cmd | getline line) > 0) print \"L:\" line
  close(cmd)
}'"

section "[14] ENVIRON inspection"
run "AWKRS_DEMO_TOKEN=awesome $AWKRS 'BEGIN { print ENVIRON[\"AWKRS_DEMO_TOKEN\"] }'"

section "[15] Parallel record mode (-j N) — order preserved"
run "command seq 1 8 | $AWKRS -j 4 '{ print \$1, \$1*\$1 }'"

section "[16] JIT vs bytecode (same binary, env toggle)"
run "command seq 1 100000 | env -u AWKRS_JIT $AWKRS '{ s += \$1 } END { print \"JIT s=\" s }'"
run "command seq 1 100000 | env AWKRS_JIT=0 $AWKRS '{ s += \$1 } END { print \"VM  s=\" s }'"

command printf '\n%sDone. See scripts/benchmark-vs-awk.sh for cross-engine timing.%s\n' "$C_HDR" "$C_OFF"
