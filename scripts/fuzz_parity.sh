#!/usr/bin/env bash
# fuzz_parity.sh — differential harness: awkrs vs a reference awk.
#
# Runs every case in a corpus under BOTH the reference awk (ground truth) and
# awkrs, and reports each case whose **stdout bytes** or **exit status** differ.
# Those are parity gaps. stderr is deliberately not compared: every awk words its
# diagnostics differently, so a byte comparison of stderr would report wording,
# not behavior — the exit status is what makes an error observable to a script.
#
#   bash scripts/fuzz_parity.sh                     # curated probes, gawk
#   bash scripts/fuzz_parity.sh -r bsd              # ... against one-true-awk
#   bash scripts/fuzz_parity.sh -r all              # gawk, then mawk, then bsd
#   bash scripts/fuzz_parity.sh -n 500 -s 7         # 500 generated cases, seed 7
#   bash scripts/fuzz_parity.sh -n 500 -P           # generated cases only
#   bash scripts/fuzz_parity.sh -c target/fuzz/PID/corpus.awkc  # re-check a corpus
#   bash scripts/fuzz_parity.sh -v                  # print every case, not just fails
#
#   -n N       generated cases to append to the corpus (default 0 — probes only)
#   -s SEED    generator seed (default 1); same seed => same corpus
#   -r REF     reference: gawk (default), mawk, bsd, or all
#   -c FILE    use this corpus instead of probes + generator
#   -P         skip the curated probes (generated cases only)
#   -t SECS    per-process timeout (default 10)
#   -F         feed the input as a *file argument* instead of on stdin
#   -v         verbose: report OK cases too
#   -h         this message
#
# `-F` exists because awk reads a named file and a pipe through different code:
# awkrs mmaps/slurps a file argument and splits fields lazily, while stdin is
# streamed. Every case here is fed on stdin by default — and so is every case in
# `parity/run_parity.sh` — which left the whole file path unexercised. It was
# hiding a record-destroying bug: assigning any field before reading one dropped
# every other field, because the deferred split had not run yet. Run both modes.
#
# Corpus format (scripts/fuzz/probes.awkc documents it in full):
#   #=== <name> / #--- prog / #--- in / #--- only <ref>[,<ref>]
#
# Each case runs in its own working directory holding an `input.txt` fixture, so
# getline-from-file cases have something deterministic to read.
#
# Exit status is the number of diverging cases, plus oracle failures, plus any
# case that never ran (0 = parity), capped at 250. A case counts only when the
# reference produced an answer: if the reference itself times out it is reported
# as ORACLE-FAIL and scored neither way, so a case where both awks hang can
# never be mistaken for a pass. The summary line accounts for every case in the
# corpus, and a shortfall is scored as failure — otherwise a run whose case tree
# went missing would print `PARITY: 0/0 cases match` and exit 0.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 2
ROOT=$(pwd)
export LC_ALL=C LANG=C

N=0 SEED=1 REF=gawk NO_PROBES=0 TMO=10 VERBOSE=0 FILE_INPUT=0
CORPUS=''
while [ $# -gt 0 ]; do
  case "$1" in
    -n) N="$2"; shift 2 ;;
    -s) SEED="$2"; shift 2 ;;
    -r) REF="$2"; shift 2 ;;
    -c) CORPUS="$2"; shift 2 ;;
    -P) NO_PROBES=1; shift ;;
    -F) FILE_INPUT=1; shift ;;
    -t) TMO="$2"; shift 2 ;;
    -v) VERBOSE=1; shift ;;
    -h) grep '^#' "$0" | cut -c3-; exit 0 ;;
    *) echo "fuzz_parity: unknown flag $1" >&2; exit 2 ;;
  esac
done

if [ -t 1 ]; then C=$'\033[36m'; G=$'\033[32m'; R=$'\033[31m'; N_=$'\033[0m'
else C=''; G=''; R=''; N_=''; fi
say() { printf '%s==>%s %s\n' "$C" "$N_" "$1"; }

AWKRS="${AWKRS:-$ROOT/target/debug/awkrs}"
[ -x "$AWKRS" ] || { echo "fuzz_parity: no binary at $AWKRS — run 'cargo build' first" >&2; exit 2; }

# A case that wedges must not wedge the harness. GNU `timeout` is `gtimeout` on
# a stock macOS with coreutils; without either, cases run unbounded and -t is
# reported as inactive rather than silently ignored.
TIMEOUT=''
if command -v timeout >/dev/null 2>&1; then TIMEOUT=timeout
elif command -v gtimeout >/dev/null 2>&1; then TIMEOUT=gtimeout
else echo "fuzz_parity: no timeout(1) — -t $TMO not enforced" >&2; fi
# shellcheck disable=SC2086  # deliberate word split: empty => no timeout wrapper
run_bounded() { ${TIMEOUT:+$TIMEOUT "$TMO"} "$@"; }

resolve_ref() {
  case "$1" in
    gawk) command -v "${GAWK:-gawk}" 2>/dev/null ;;
    mawk) command -v "${MAWK:-mawk}" 2>/dev/null ;;
    bsd)
      if [ -n "${BSD_AWK:-}" ]; then command -v "$BSD_AWK" 2>/dev/null
      elif command -v nawk >/dev/null 2>&1; then command -v nawk
      elif command -v original-awk >/dev/null 2>&1; then command -v original-awk
      elif [ -x /usr/bin/awk ] && [ "$(uname -s)" = Darwin ]; then echo /usr/bin/awk
      fi ;;
    *) return 1 ;;
  esac
}

# Each run gets its own scratch directory. The path used to be a fixed
# `target/fuzz`, which two concurrent invocations in the same checkout silently
# destroyed for each other: the second run's `rm -rf` deleted the case tree the
# first was still iterating, so the first reported "PARITY: 0/0 cases match" —
# a wiped corpus reading as a clean run. `$$` is the shell's own pid, so runs
# cannot collide; the directory is left in place afterwards because divergence
# reports cite paths inside it.
OUT=$ROOT/target/fuzz/$$
command rm -rf "$OUT"
mkdir -p "$OUT/cases" "$OUT/work"
say "scratch: $OUT"

# ── build the corpus ────────────────────────────────────────────────────────
if [ -n "$CORPUS" ]; then
  cp "$CORPUS" "$OUT/corpus.awkc"
  say "using corpus $CORPUS"
else
  : >"$OUT/corpus.awkc"
  if [ "$NO_PROBES" -eq 0 ]; then command cat scripts/fuzz/probes.awkc >>"$OUT/corpus.awkc"; fi
  if [ "$N" -gt 0 ]; then
    say "generating $N cases (seed $SEED)"
    perl scripts/fuzz/gen_awk.pl "$SEED" "$N" >>"$OUT/corpus.awkc" || exit 2
  fi
fi

# Split into one directory per case: prog.awk, in.txt, only.
perl -e '
  my ($corpus, $dir) = @ARGV;
  open my $c, "<:raw", $corpus or die "corpus: $!";
  my ($name, $sec, $n, %buf) = (undef, undef, 0);
  my $flush = sub {
    return unless defined $name;
    my $d = sprintf("%s/%05d", $dir, $n);
    mkdir $d or die "mkdir $d: $!";
    open my $f, ">:raw", "$d/name" or die; print {$f} "$name\n"; close $f;
    for my $k (qw(prog in only)) {
      next unless defined $buf{$k};
      my %f = (prog => "prog.awk", in => "in.txt", only => "only");
      # Drop exactly one trailing empty line: the corpus puts a blank line
      # between cases for readability and it is not part of the data. A case
      # that genuinely needs to end in a blank line writes two.
      $buf{$k} =~ s/\n\n\z/\n/ if $k ne "only";
      open my $g, ">:raw", "$d/$f{$k}" or die; print {$g} $buf{$k}; close $g;
    }
    %buf = ();
  };
  while (my $l = <$c>) {
    if ($l =~ /^\#=== (\S+)/) { $flush->(); $name = $1; $n++; $sec = undef; next }
    if ($l =~ /^\#--- only\s+(\S+)/) { $buf{only} = "$1\n"; next }
    if ($l =~ /^\#--- (prog|in)\s*$/) { $sec = $1; next }
    next if !defined $name || !defined $sec;
    $buf{$sec} .= $l;
  }
  $flush->();
  print "$n\n";
' "$OUT/corpus.awkc" "$OUT/cases" >"$OUT/count" || exit 2
TOTAL=$(command cat "$OUT/count")
[ "$TOTAL" -gt 0 ] || { echo "fuzz_parity: empty corpus" >&2; exit 2; }

# ── run one reference over the whole corpus ─────────────────────────────────
run_ref() {
  local refname=$1 refbin=$2
  local pass=0 skip=0 diverge=0 oracle=0
  say "$TOTAL cases: $refbin (reference) vs $AWKRS"

  local case name only work rexit sexit
  for case in "$OUT/cases"/*; do
    [ -d "$case" ] || continue
    name=$(command cat "$case/name")
    only=""
    [ -f "$case/only" ] && only=$(command cat "$case/only")
    if [ -n "$only" ] && ! printf '%s' ",$only," | grep -q ",$refname,"; then
      skip=$((skip + 1)); continue
    fi

    work="$OUT/work/$refname/$(basename "$case")"
    mkdir -p "$work"
    printf 'L1\nL2\nL3\n' >"$work/input.txt"
    [ -f "$case/in.txt" ] && cp "$case/in.txt" "$work/stdin" || : >"$work/stdin"

    if [ "$FILE_INPUT" -eq 1 ]; then
      # Same bytes, handed over as a named operand instead of a pipe. Both awks
      # see the same FILENAME, so anything printing it still compares equal.
      ( builtin cd "$work" && run_bounded "$refbin" -f "$case/prog.awk" stdin >ref.out 2>ref.err )
      rexit=$?
      ( builtin cd "$work" && run_bounded "$AWKRS" -f "$case/prog.awk" stdin >sub.out 2>sub.err )
      sexit=$?
    else
      ( builtin cd "$work" && run_bounded "$refbin" -f "$case/prog.awk" <stdin >ref.out 2>ref.err )
      rexit=$?
      ( builtin cd "$work" && run_bounded "$AWKRS" -f "$case/prog.awk" <stdin >sub.out 2>sub.err )
      sexit=$?
    fi

    # A case only counts once the *reference* actually produced an answer. When
    # the oracle is killed by the timeout (124 from `timeout`, 137 from a
    # follow-up KILL) it has no opinion, and if awkrs happens to hang too the
    # two empty outputs and equal statuses would compare equal and be scored a
    # pass. Those are reported on their own line instead, never as parity.
    if [ "$rexit" = 124 ] || [ "$rexit" = 137 ]; then
      oracle=$((oracle + 1))
      printf '%sORACLE-FAIL%s [%s] (%s) reference timed out after %ss; awkrs exit=%s — not scored\n' \
        "$R" "$N_" "$name" "$refname" "$TMO" "$sexit"
      continue
    fi

    if cmp -s "$work/ref.out" "$work/sub.out" && [ "$rexit" = "$sexit" ]; then
      pass=$((pass + 1))
      [ "$VERBOSE" -eq 1 ] && printf '  %sok%s   %s\n' "$G" "$N_" "$name"
    else
      diverge=$((diverge + 1))
      printf '%sDIVERGE%s [%s] (%s)\n' "$R" "$N_" "$name" "$refname"
      printf -- '--- program ---\n'; command cat "$case/prog.awk"
      [ -s "$work/stdin" ] && { printf -- '--- stdin ---\n'; command cat "$work/stdin"; }
      printf -- '--- %s: exit=%s ---\n' "$refname" "$rexit"; command cat "$work/ref.out"
      [ -s "$work/ref.err" ] && perl -pe 's/^/  stderr: /' "$work/ref.err"
      printf -- '--- awkrs: exit=%s ---\n' "$sexit"; command cat "$work/sub.out"
      [ -s "$work/sub.err" ] && perl -pe 's/^/  stderr: /' "$work/sub.err"
      printf -- '--- stdout diff (%s vs awkrs) ---\n' "$refname"
      diff -u "$work/ref.out" "$work/sub.out" || true
      echo
    fi
  done

  # Every case in the corpus must land in exactly one bucket. Printing the
  # accounting makes a silent shortfall — cases lost to a malformed corpus, say
  # — visible instead of shrinking the denominator that "N/N match" is measured
  # against.
  local scored=$((pass + diverge))
  if [ "$diverge" -eq 0 ]; then
    printf '%sPARITY (%s): %d/%d cases match%s' "$G" "$refname" "$pass" "$scored" "$N_"
  else
    printf '%s%d of %d cases diverge from %s%s' "$R" "$diverge" "$scored" "$refname" "$N_"
  fi
  printf ' (%d skipped by `only`, %d oracle failures, %d of %d accounted)\n' \
    "$skip" "$oracle" "$((scored + skip + oracle))" "$TOTAL"
  # A shortfall is a failure, not a note. Cases that never ran shrink the
  # denominator "N/N match" is measured against, so a run that lost its whole
  # corpus used to print `PARITY: 0/0 cases match` and exit 0 — the strongest
  # possible green light for the weakest possible evidence. Counting the
  # missing cases as failures makes an unusable run exit non-zero.
  local missing=$((TOTAL - scored - skip - oracle))
  if [ "$missing" -ne 0 ]; then
    printf '%sWARNING%s: %d case(s) in the corpus were never run — the corpus is malformed\n' \
      "$R" "$N_" "$missing"
  fi
  # Oracle failures are not parity, so they must not read as success either.
  return "$((diverge + oracle + missing))"
}

REFS=$REF
[ "$REF" = all ] && REFS="gawk mawk bsd"

status=0
for r in $REFS; do
  bin=$(resolve_ref "$r") || { echo "fuzz_parity: unknown reference '$r'" >&2; exit 2; }
  if [ -z "$bin" ]; then echo "fuzz_parity: no '$r' awk on PATH — skipping" >&2; continue; fi
  run_ref "$r" "$bin" || status=$((status + $?))
done

[ "$status" -gt 250 ] && status=250
exit "$status"
