#!/usr/bin/env bash
# README §1–§10 workloads: awkrs JIT (default) vs bytecode only (AWKRS_JIT=0).
# Matches programs in README.md [0x06] ### 1 … ### 10; input is 200 K lines unless noted.
# Requires: cargo, hyperfine. Writes benchmarks/benchmark-readme-jit.md
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT" || exit 1
command mkdir -p "$ROOT/benchmarks"

echo "Building awkrs --release..."
command cargo build --release -q

AWKRS="$ROOT/target/release/awkrs"
TMP1="$(command mktemp "${TMPDIR:-/tmp}/awkrs-rj1.XXXXXX")"
TMP5F="$(command mktemp "${TMPDIR:-/tmp}/awkrs-rj5.XXXXXX")"
TMP2F="$(command mktemp "${TMPDIR:-/tmp}/awkrs-rj2.XXXXXX")"
trap 'command rm -f "$TMP1" "$TMP5F" "$TMP2F"' EXIT

# 200 K lines: one field (§1, §3, §5, §10); five fields (§4, §6, §9); two fields (§7, §8)
command seq 1 200000 >"$TMP1"
command awk 'BEGIN{for(i=1;i<=200000;i++) print i, i+1, i+2, i+3, i+4}' >"$TMP5F"
command awk 'BEGIN{for(i=1;i<=200000;i++) print i, i*2}' >"$TMP2F"

OUT="$ROOT/benchmarks/benchmark-readme-jit.md"

append_hf_markdown() {
  local md
  md="$(command mktemp "${TMPDIR:-/tmp}/awkrs-hf-rj.XXXXXX")"
  command hyperfine --style none --warmup 2 --min-runs 8 "$@" --export-markdown "$md" || {
    command rm -f "$md"
    return 1
  }
  command cat "$md" >>"$OUT"
  command rm -f "$md"
}

{
  echo "# awkrs README: JIT vs bytecode (§1–§10)"
  echo ""
  echo "This file is **generated** by \`./scripts/benchmark-readme-jit-vs-vm.sh\`. Do not edit by hand."
  echo ""
  echo "Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin)."
  echo ""
  echo "## Environment"
  echo ""
  echo "- **Generated at (UTC):** $(command date -u '+%Y-%m-%d %H:%M:%S')"
  echo "- **uname:** \`$(command uname -srm)\`"
  if command -v sysctl >/dev/null 2>&1; then
    echo "- **CPU (macOS sysctl):** $(command sysctl -n machdep.cpu.brand_string 2>/dev/null || echo n/a)"
  fi
  echo "- **awkrs:** \`$AWKRS\` (\`$(command "$AWKRS" --version 2>/dev/null | command head -1)\`)"
  echo ""
  echo "**JIT on:** \`env -u AWKRS_JIT …\` — **JIT off:** \`AWKRS_JIT=0 …\`"
  echo ""
} >"$OUT"

jit_pair() {
  local title="$1"
  shift
  {
    echo "## $title"
    echo ""
  } >>"$OUT"
  append_hf_markdown "$@"
  echo "" >>"$OUT"
}

jit_pair "1. Throughput: \`{ print \$1 }\` (one field per line)" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ print \$1 }' '$TMP1'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ print \$1 }' '$TMP1'"

jit_pair "2. CPU-bound BEGIN (no input)" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" 'BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }' </dev/null"

jit_pair "3. Sum first column: \`{ s += \$1 } END { print s }\`" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ s += \$1 } END { print s }' '$TMP1'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ s += \$1 } END { print s }' '$TMP1'"

jit_pair "4. Multi-field print: \`{ print \$1, \$3, \$5 }\` (five fields per line)" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ print \$1, \$3, \$5 }' '$TMP5F'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ print \$1, \$3, \$5 }' '$TMP5F'"

jit_pair "5. Regex filter: \`/alpha/ { c += 1 } END { print c }\` (lines have no \`alpha\`)" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '/alpha/ { c += 1 } END { print c }' '$TMP1'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '/alpha/ { c += 1 } END { print c }' '$TMP1'"

jit_pair "6. Associative array: \`{ a[\$5] += 1 } END { for (k in a) print k, a[k] }\`" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ a[\$5] += 1 } END { for (k in a) print k, a[k] }' '$TMP5F'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ a[\$5] += 1 } END { for (k in a) print k, a[k] }' '$TMP5F'"

jit_pair "7. Conditional field: \`NR % 2 == 0 { print \$2 }\`" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" 'NR % 2 == 0 { print \$2 }' '$TMP2F'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" 'NR % 2 == 0 { print \$2 }' '$TMP2F'"

jit_pair "8. Field computation: \`{ sum += \$1 * \$2 } END { print sum }\`" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ sum += \$1 * \$2 } END { print sum }' '$TMP2F'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ sum += \$1 * \$2 } END { print sum }' '$TMP2F'"

jit_pair "9. String concat print: \`{ print \$3 \"-\" \$5 }\`" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ print \$3 \"-\" \$5 }' '$TMP5F'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ print \$3 \"-\" \$5 }' '$TMP5F'"

jit_pair "10. gsub: \`{ gsub(\"alpha\", \"ALPHA\"); print }\` (no \`alpha\` in input)" \
  -n "awkrs (JIT default)" "env -u AWKRS_JIT \"$AWKRS\" '{ gsub(\"alpha\", \"ALPHA\"); print }' '$TMP1'" \
  -n "awkrs (bytecode only)" "env AWKRS_JIT=0 \"$AWKRS\" '{ gsub(\"alpha\", \"ALPHA\"); print }' '$TMP1'"

{
  echo ""
  echo "---"
  echo ""
  echo "Re-run after \`cargo build --release\` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [\`benchmark-results.md\`](benchmark-results.md) from \`./scripts/benchmark-vs-awk.sh\`."
  echo ""
} >>"$OUT"

echo "Wrote $OUT"
