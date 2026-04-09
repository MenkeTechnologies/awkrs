#!/usr/bin/env bash
# Compare awkrs (release) to system awk and gawk (if present). Writes benchmarks/benchmark-results.md.
# Requires: cargo, hyperfine (https://github.com/sharkdp/hyperfine). Optional: gawk.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
command mkdir -p "$ROOT/benchmarks"

echo "Building awkrs --release..."
command cargo build --release -q

AWKRS="$ROOT/target/release/awkrs"
AWK_BIN="${AWK:-/usr/bin/awk}"
GAWK_BIN="$(command -v gawk 2>/dev/null || true)"

TMP_LINES="$(command mktemp "${TMPDIR:-/tmp}/awkrs-bench-lines.XXXXXX")"
trap 'command rm -f "$TMP_LINES"' EXIT

# Deterministic input: one integer field per line
command seq 1 200000 >"$TMP_LINES"

OUT="$ROOT/benchmarks/benchmark-results.md"
{
  echo "# awkrs vs awk benchmarks"
  echo ""
  echo "This file is **generated** by \`./scripts/benchmark-vs-awk.sh\`. Do not edit by hand."
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
  echo "- **awkrs:** \`$AWKRS\` (\`$(command "$AWKRS" --version 2>/dev/null | command head -1)\`)"
  echo ""

  echo "## 1. Throughput: print first field"
  echo ""
  echo 'Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.'
  echo ""
} >"$OUT"

append_hf_markdown() {
  local md
  md="$(command mktemp "${TMPDIR:-/tmp}/awkrs-hf.XXXXXX")"
  command hyperfine --style none --warmup 2 --min-runs 8 "$@" --export-markdown "$md" || {
    command rm -f "$md"
    return 1
  }
  command cat "$md" >>"$OUT"
  command rm -f "$md"
}

if [[ -n "$GAWK_BIN" ]]; then
  append_hf_markdown \
    -n "BSD awk" "$AWK_BIN '{ print \$1 }' '$TMP_LINES'" \
    -n "gawk" "$GAWK_BIN '{ print \$1 }' '$TMP_LINES'" \
    -n "awkrs" "$AWKRS '{ print \$1 }' '$TMP_LINES'" \
    -n "awkrs (parallel)" "$AWKRS -j8 '{ print \$1 }' '$TMP_LINES'"
else
  append_hf_markdown \
    -n "awk" "$AWK_BIN '{ print \$1 }' '$TMP_LINES'" \
    -n "awkrs" "$AWKRS '{ print \$1 }' '$TMP_LINES'" \
    -n "awkrs (parallel)" "$AWKRS -j8 '{ print \$1 }' '$TMP_LINES'"
fi

{
  echo ""
  echo "## 2. CPU-bound BEGIN (no input)"
  echo ""
  echo "Program: \`BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }\` (stdin empty; \`<\` avoids a parser limitation on \`<=\` in this \`for\`)."
  echo ""
} >>"$OUT"

if [[ -n "$GAWK_BIN" ]]; then
  append_hf_markdown \
    -n "BSD awk" "$AWK_BIN 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null" \
    -n "gawk" "$GAWK_BIN 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null" \
    -n "awkrs" "$AWKRS 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null"
else
  append_hf_markdown \
    -n "awk" "$AWK_BIN 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null" \
    -n "awkrs" "$AWKRS 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null"
fi

{
  echo ""
  echo "## 3. Sum first column (single-threaded)"
  echo ""
  echo 'Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is single-threaded by default here.)'
  echo ""
} >>"$OUT"

if [[ -n "$GAWK_BIN" ]]; then
  append_hf_markdown \
    -n "BSD awk" "$AWK_BIN '{ s += \$1 } END { print s }' '$TMP_LINES'" \
    -n "gawk" "$GAWK_BIN '{ s += \$1 } END { print s }' '$TMP_LINES'" \
    -n "awkrs" "$AWKRS '{ s += \$1 } END { print s }' '$TMP_LINES'"
else
  append_hf_markdown \
    -n "awk" "$AWK_BIN '{ s += \$1 } END { print s }' '$TMP_LINES'" \
    -n "awkrs" "$AWKRS '{ s += \$1 } END { print s }' '$TMP_LINES'"
fi

{
  echo ""
  echo "## 4. awkrs: JIT vs bytecode VM"
  echo ""
  echo "Same **awkrs** binary: default path (JIT attempted for eligible chunks) vs \`AWKRS_JIT=0\` (bytecode interpreter only). Use \`env -u AWKRS_JIT\` so a shell \`AWKRS_JIT\` alias does not skew the “JIT on” run."
  echo ""
  echo "### 4a. CPU-bound BEGIN (same program as §2)"
  echo ""
} >>"$OUT"

append_hf_markdown \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null"

{
  echo ""
  echo "### 4b. Sum first column (same program and input as §3)"
  echo ""
} >>"$OUT"

append_hf_markdown \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ s += \$1 } END { print s }' '$TMP_LINES'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ s += \$1 } END { print s }' '$TMP_LINES'"

{
  echo ""
  echo "### 4c. Throughput: print first field (same program and input as §1)"
  echo ""
} >>"$OUT"

append_hf_markdown \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ print \$1 }' '$TMP_LINES'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ print \$1 }' '$TMP_LINES'"

{
  echo ""
  echo "### 4d. Parallel \`-j8\` (same program and input as §1 \`awkrs (parallel)\`)"
  echo ""
} >>"$OUT"

append_hf_markdown \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" -j8 '{ print \$1 }' '$TMP_LINES'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" -j8 '{ print \$1 }' '$TMP_LINES'"

{
  echo ""
  echo "---"
  echo ""
  echo "Throughput (§1) can use **awkrs \`-j\`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. **§4** compares JIT vs bytecode for every **awkrs** workload in §1–§3 (4a = §2, 4b = §3, 4c–4d = §1). Re-run after \`cargo build --release\` on your hardware."
  echo ""
} >>"$OUT"

echo "Wrote $OUT"
