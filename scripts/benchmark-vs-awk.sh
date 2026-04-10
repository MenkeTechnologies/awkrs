#!/usr/bin/env bash
# Compare awkrs (release) to awk/gawk/mawk across README §1–§10 workloads.
# Each §N is a SINGLE hyperfine invocation including BSD awk, gawk (if present),
# mawk (if present), awkrs (JIT default) and awkrs (AWKRS_JIT=0 bytecode only),
# so the "Relative" column in every table is apples-to-apples.
# Requires: cargo, hyperfine (https://github.com/sharkdp/hyperfine). Optional: gawk, mawk.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT" || exit 1
command mkdir -p "$ROOT/benchmarks"

echo "Building awkrs --release..."
command cargo build --release -q

AWKRS="$ROOT/target/release/awkrs"
AWK_BIN="${AWK:-/usr/bin/awk}"
GAWK_BIN="$(command -v gawk 2>/dev/null || true)"
MAWK_BIN="$(command -v mawk 2>/dev/null || true)"

TMP1="$(command mktemp "${TMPDIR:-/tmp}/awkrs-bench1.XXXXXX")"
TMP5F="$(command mktemp "${TMPDIR:-/tmp}/awkrs-bench5.XXXXXX")"
TMP2F="$(command mktemp "${TMPDIR:-/tmp}/awkrs-bench2.XXXXXX")"
trap 'command rm -f "$TMP1" "$TMP5F" "$TMP2F"' EXIT

# 1 M lines (overridable via $AWKRS_BENCH_LINES) — enough to lift short workloads
# like `{ print $1 }` out of hyperfine's sub-5 ms shell-startup noise floor.
# One field (§1,§3,§5,§10), five fields (§4,§6,§9), two fields (§7,§8).
LINES="${AWKRS_BENCH_LINES:-1000000}"
command seq 1 "$LINES" >"$TMP1"
"$AWK_BIN" -v n="$LINES" 'BEGIN{for(i=1;i<=n;i++) print i, i+1, i+2, i+3, i+4}' >"$TMP5F"
"$AWK_BIN" -v n="$LINES" 'BEGIN{for(i=1;i<=n;i++) print i, i*2}' >"$TMP2F"
LINES_LABEL="$(command printf '%s' "$LINES" | "$AWK_BIN" '{
  if ($1 >= 1000000) printf "%g M", $1/1000000
  else if ($1 >= 1000) printf "%g K", $1/1000
  else print $1
}')"

OUT="$ROOT/benchmarks/benchmark-results.md"
{
  echo "# awkrs vs awk benchmarks"
  echo ""
  echo "This file is **generated** by \`./scripts/benchmark-vs-awk.sh\`. Do not edit by hand."
  echo ""
  echo "Each \`§N\` section below is a **single** \`hyperfine\` invocation with every available engine (BSD awk, gawk, mawk, awkrs JIT-default, awkrs \`AWKRS_JIT=0\` bytecode-only) on the same input, so the *Relative* column is apples-to-apples within each table. Input size: **${LINES_LABEL}** lines (override with \`AWKRS_BENCH_LINES=500000 ./scripts/benchmark-vs-awk.sh\`). Sizes smaller than ~500 K will put \`{ print \$1 }\`-style workloads below hyperfine's shell-startup noise floor (< 5 ms), so the mean becomes unreliable even with more runs. Workloads mirror [README.md](../README.md) **[0x06] BENCHMARKS** §1–§10. For the focused awkrs-only JIT vs bytecode A/B (same programs), see [\`benchmark-readme-jit.md\`](benchmark-readme-jit.md) from \`./scripts/benchmark-readme-jit-vs-vm.sh\`."
  echo ""
  echo "## Environment"
  echo ""
  echo "- **Generated at (UTC):** $(command date -u '+%Y-%m-%d %H:%M:%S')"
  echo "- **uname:** \`$(command uname -srm)\`"
  if command -v sysctl >/dev/null 2>&1; then
    echo "- **CPU (macOS sysctl):** $(command sysctl -n machdep.cpu.brand_string 2>/dev/null || echo n/a)"
  fi
  echo "- **awk:** \`$AWK_BIN\`"
  if [[ -n "$GAWK_BIN" ]]; then
    echo "- **gawk:** \`$GAWK_BIN\` (\`$(command "$GAWK_BIN" --version 2>/dev/null | command head -1)\`)"
  else
    echo "- **gawk:** not found on PATH"
  fi
  if [[ -n "$MAWK_BIN" ]]; then
    echo "- **mawk:** \`$MAWK_BIN\` (\`$(command "$MAWK_BIN" -W version 2>&1 | command head -1)\`)"
  else
    echo "- **mawk:** not found on PATH"
  fi
  echo "- **awkrs:** \`$AWKRS\` (\`$(command "$AWKRS" --version 2>/dev/null | command head -1)\`)"
  echo ""
  echo "**JIT on:** \`env -u AWKRS_JIT …\` — **JIT off:** \`env AWKRS_JIT=0 …\` (same binary)."
  echo ""
} >"$OUT"

append_hf_markdown() {
  local md
  md="$(command mktemp "${TMPDIR:-/tmp}/awkrs-hf.XXXXXX")"
  command hyperfine --style none --warmup 3 --min-runs 10 "$@" --export-markdown "$md" || {
    command rm -f "$md"
    return 1
  }
  command cat "$md" >>"$OUT"
  command rm -f "$md"
}

# bench_cross "title" "<awk program>" "<input file or empty for </dev/null>"
bench_cross() {
  local title="$1"
  local prog="$2"
  local input="${3:-}"
  local inputQ redir
  if [[ -z "$input" ]]; then
    inputQ=""
    redir=" </dev/null"
  else
    inputQ=" '$input'"
    redir=""
  fi
  {
    echo "## $title"
    echo ""
  } >>"$OUT"
  local -a args=()
  args+=(-n "BSD awk"         "$AWK_BIN '$prog'${inputQ}${redir}")
  if [[ -n "$GAWK_BIN" ]]; then
    args+=(-n "gawk"          "$GAWK_BIN '$prog'${inputQ}${redir}")
  fi
  if [[ -n "$MAWK_BIN" ]]; then
    args+=(-n "mawk"          "$MAWK_BIN '$prog'${inputQ}${redir}")
  fi
  args+=(-n "awkrs (JIT)"      "env -u AWKRS_JIT \"$AWKRS\" '$prog'${inputQ}${redir}")
  args+=(-n "awkrs (bytecode)" "env AWKRS_JIT=0 \"$AWKRS\" '$prog'${inputQ}${redir}")
  append_hf_markdown "${args[@]}"
  echo "" >>"$OUT"
}

bench_cross "1. Throughput: \`{ print \$1 }\` (${LINES_LABEL} × 1 field)" \
  '{ print $1 }' "$TMP1"

bench_cross "2. CPU-bound BEGIN (no input, 400 K-iter loop)" \
  'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' ""

bench_cross "3. Sum first column: \`{ s += \$1 } END { print s }\` (${LINES_LABEL} × 1 field)" \
  '{ s += $1 } END { print s }' "$TMP1"

bench_cross "4. Multi-field print: \`{ print \$1, \$3, \$5 }\` (${LINES_LABEL} × 5 fields)" \
  '{ print $1, $3, $5 }' "$TMP5F"

bench_cross "5. Regex filter: \`/alpha/ { c += 1 } END { print c }\` (${LINES_LABEL}, no matches)" \
  '/alpha/ { c += 1 } END { print c }' "$TMP1"

bench_cross "6. Associative array: \`{ a[\$5] += 1 } END { for (k in a) print k, a[k] }\` (${LINES_LABEL} × 5 fields)" \
  '{ a[$5] += 1 } END { for (k in a) print k, a[k] }' "$TMP5F"

bench_cross "7. Conditional field: \`NR % 2 == 0 { print \$2 }\` (${LINES_LABEL} × 2 fields)" \
  'NR % 2 == 0 { print $2 }' "$TMP2F"

bench_cross "8. Field computation: \`{ sum += \$1 * \$2 } END { print sum }\` (${LINES_LABEL} × 2 fields)" \
  '{ sum += $1 * $2 } END { print sum }' "$TMP2F"

bench_cross "9. String concat print: \`{ print \$3 \"-\" \$5 }\` (${LINES_LABEL} × 5 fields)" \
  '{ print $3 "-" $5 }' "$TMP5F"

bench_cross "10. gsub: \`{ gsub(\"alpha\", \"ALPHA\"); print }\` (${LINES_LABEL} × 1 field, no matches)" \
  '{ gsub("alpha", "ALPHA"); print }' "$TMP1"

{
  echo "---"
  echo ""
  echo "Re-run after \`cargo build --release\` on your hardware. Install mawk (\`brew install mawk\` / \`apt install mawk\`) and gawk for full cross-engine tables; without them the table simply omits that row. §6 iteration order differs across engines, so its output is not compared — only the mean time."
  echo ""
} >>"$OUT"

echo "Wrote $OUT"
