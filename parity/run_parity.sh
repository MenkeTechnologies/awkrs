#!/usr/bin/env bash
# Compare reference awk(1) vs awkrs(1) (exact stdout+stderr bytes, LC_ALL=C).
#
# Usage (repo root):
#   bash parity/run_parity.sh           # same as gawk
#   bash parity/run_parity.sh gawk      # parity/cases/*.awk + parity/cases_portable/*.awk
#   bash parity/run_parity.sh mawk      # same case set as gawk (different reference awk)
#   bash parity/run_parity.sh bsd       # same case set as gawk; see BSD_AWK below
#   bash parity/run_parity.sh all       # gawk, then mawk, then bsd (first failure exits1)
#
# Env:
#   AWKRS=path/to/awkrs
#   GAWK=gawk   MAWK=mawk
#   BSD_AWK=    Reference for bsd mode: if unset, try nawk, then original-awk (Linux), else /usr/bin/awk (Darwin).

set -euo pipefail

ROOT="$(builtin cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export LC_ALL=C
export LANG=C

AWKRS="${AWKRS:-$ROOT/target/release/awkrs}"

resolve_bsd_awk() {
  if [[ -n "${BSD_AWK:-}" ]]; then
    printf '%s' "$BSD_AWK"
    return
  fi
  if command -v nawk >/dev/null 2>&1; then
    printf '%s' "nawk"
    return
  fi
  if command -v original-awk >/dev/null 2>&1; then
    printf '%s' "original-awk"
    return
  fi
  if [[ "$(uname -s)" == Darwin ]] && [[ -x /usr/bin/awk ]]; then
    printf '%s' "/usr/bin/awk"
    return
  fi
  echo "parity: set BSD_AWK to a BSD awk (e.g. nawk or original-awk on Linux; macOS /usr/bin/awk works)" >&2
  return 1
}

run_one_ref() {
  local ref_name=$1
  local ref_cmd=$2
  shift 2
  local cases=("$@")

  if [[ ${#cases[@]} -eq 0 ]]; then
    echo "parity ($ref_name): no case files matched" >&2
    return 2
  fi

  if ! command -v "$ref_cmd" >/dev/null 2>&1 && [[ ! -x "$ref_cmd" ]]; then
    echo "parity ($ref_name): '$ref_cmd' not found" >&2
    return 2
  fi

  local failed=0
  local f base stem inp dat p_out r_out
  for f in "${cases[@]}"; do
    base=$(basename "$f")
    stem="${f%.awk}"
    inp="${stem}.in"
    dat="${stem}.dat"

    if [[ -f "$inp" && -f "$dat" ]]; then
      echo "parity: $base: both .in and .dat exist; use one input mode" >&2
      return 2
    fi

    p_out=$(mktemp "${TMPDIR:-/tmp}/parity.ref.$$.XXXXXX")
    r_out=$(mktemp "${TMPDIR:-/tmp}/parity.awkrs.$$.XXXXXX")

    if [[ -f "$dat" ]]; then
      "$ref_cmd" -f "$f" "$dat" >"$p_out" 2>&1 || true
      "$AWKRS" -f "$f" "$dat" >"$r_out" 2>&1 || true
    elif [[ -f "$inp" ]]; then
      "$ref_cmd" -f "$f" <"$inp" >"$p_out" 2>&1 || true
      "$AWKRS" -f "$f" <"$inp" >"$r_out" 2>&1 || true
    else
      "$ref_cmd" -f "$f" </dev/null >"$p_out" 2>&1 || true
      "$AWKRS" -f "$f" </dev/null >"$r_out" 2>&1 || true
    fi

    if ! cmp -s "$p_out" "$r_out"; then
      echo "parity FAIL ($ref_name): $base" >&2
      echo "--- $ref_cmd $base ---" >&2
      command cat "$p_out" >&2
      echo "--- $AWKRS $base ---" >&2
      command cat "$r_out" >&2
      echo "--- diff ($ref_name vs awkrs) ---" >&2
      diff -u "$p_out" "$r_out" >&2 || true
      failed=$((failed + 1))
    else
      echo "parity OK ($ref_name): $base"
    fi

    command rm -f "$p_out" "$r_out"
  done

  if [[ "$failed" -ne 0 ]]; then
    echo "parity ($ref_name): $failed case(s) mismatch" >&2
    return 1
  fi
  echo "parity ($ref_name): all ${#cases[@]} case(s) match"
  return 0
}

ensure_awkrs() {
  if [[ ! -x "$AWKRS" ]]; then
    echo "parity: building release awkrs (cargo build --release)…" >&2
    (builtin cd "$ROOT" && cargo build --release --locked -q)
  fi
  if [[ ! -x "$AWKRS" ]]; then
    echo "parity: no executable at AWKRS=$AWKRS" >&2
    exit 2
  fi
}

run_mode() {
  local mode=$1
  local ref_cmd ref_name
  local -a cases
  local _ng
  # Bash 5.3+: `shopt -p <opt>` exits 1 when that option is off; keep going under `set -e`.
  _ng=$(shopt -p nullglob) || true
  shopt -s nullglob
  case "$mode" in
  gawk)
    ref_name=gawk
    ref_cmd="${GAWK:-gawk}"
    if ! command -v "$ref_cmd" >/dev/null 2>&1; then
      eval "$_ng"
      echo "parity: '$ref_cmd' not on PATH" >&2
      return 2
    fi
    ;;
  mawk)
    ref_name=mawk
    ref_cmd="${MAWK:-mawk}"
    if ! command -v "$ref_cmd" >/dev/null 2>&1; then
      eval "$_ng"
      echo "parity: '$ref_cmd' not on PATH" >&2
      return 2
    fi
    ;;
  bsd)
    ref_name=bsd
    ref_cmd=$(resolve_bsd_awk) || { eval "$_ng"; return 2; }
    ;;
  *)
    eval "$_ng"
    echo "parity: unknown mode '$mode' (use gawk, mawk, bsd, or all)" >&2
    return 2
    ;;
  esac
  cases=( "$ROOT"/parity/cases/*.awk "$ROOT"/parity/cases_portable/*.awk )
  # Include gawk-only extension tests only when testing against gawk
  if [[ "$mode" == "gawk" ]]; then
    cases+=( "$ROOT"/parity/cases_gawk/*.awk )
  fi
  eval "$_ng"

  run_one_ref "$ref_name" "$ref_cmd" "${cases[@]}"
}

main() {
  local cmd="${1:-gawk}"

  if [[ "$cmd" == all ]]; then
    ensure_awkrs
    local ec=0
    run_mode gawk || ec=1
    run_mode mawk || ec=1
    run_mode bsd || ec=1
    exit "$ec"
  fi

  ensure_awkrs
  run_mode "$cmd"
}

main "$@"
