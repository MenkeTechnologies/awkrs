#!/usr/bin/env perl
# gen_awk.pl SEED N [--out FILE] — seeded generator for the differential corpus.
#
# Emits N awk cases in the corpus format read by scripts/fuzz_parity.sh:
#
#   #=== <name>
#   #--- prog
#   <program text>
#   #--- in
#   <stdin text>            (optional)
#
# The same SEED always produces the same corpus, so a divergence found on one
# machine reproduces exactly on another. Every generated program is written to
# be *deterministic* — no rand(), no systime(), no environment, no `system()`,
# no output whose order depends on array iteration — because the harness
# byte-compares stdout. Anything non-deterministic would show up as a fake
# divergence and drown the real ones.
#
# The templates deliberately concentrate on the semantics where awk
# implementations actually differ: field splitting, number<->string coercion,
# printf conversions, the substr/index/split/sub/gsub/match family, arrays and
# `in`/delete, and the getline forms.

use strict;
use warnings;

my ($seed, $n) = (shift // 1, shift // 200);
my $out;
while (@ARGV) {
    my $a = shift;
    if ($a eq '--out') { $out = shift; next; }
    die "gen_awk.pl: unknown argument $a\n";
}

# Small deterministic LCG — perl's own rand() is not guaranteed stable across
# builds, and the whole point of the seed is that the corpus is reproducible.
my $state = ($seed * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFF;
sub rnd {
    my ($lim) = @_;
    $state = ($state * 1103515245 + 12345) & 0x7FFFFFFF;
    return $state % $lim;
}
sub pick { my @c = @_; return $c[rnd(scalar @c)] }

# ── generator vocabulary ────────────────────────────────────────────────────
my @NUMS  = qw(0 1 -1 2 3.5 -3.5 0.1 1e3 1e-3 1e17 2147483647 2147483648 0.0000001 100000 7 -7);
# No "0x..." string here: one-true-awk's strtod accepts the hex prefix while
# gawk and mawk (and POSIX) do not, so it would report the same known reference
# disagreement on every case instead of finding new ones. That divergence is
# pinned once, deliberately, by `string_to_number_hex_literal` in probes.awkc.
my @STRS  = ('""', '"a"', '"abc"', '"hello world"', '"10"', '"10.0"', '" 12 "', '"1f"', '"-3"', '"1e3"', '"a:b:c"', '"aXbXc"', '"  spaced  "');
my @FSES  = ('" "', '":"', '","', '"\t"', '"[0-9]+"', '"[:,]"', '"ab"', '"."', '"|"');
my @RSES  = ('"\n"', '":"', '""', '"ab"');
my @REGEX = ('/a/', '/[0-9]+/', '/^a/', '/c$/', '/a.c/', '/[aeiou]/', '/x*/', '/(ab)+/');
my @FMTS  = ('"%d"', '"%i"', '"%5d"', '"%-5d|"', '"%05d"', '"%+d"', '"% d"',
             '"%s"', '"%10s|"', '"%-10s|"', '"%.3s"', '"%c"',
             '"%o"', '"%x"', '"%X"', '"%e"', '"%E"', '"%f"', '"%.2f"', '"%g"', '"%G"',
             '"%*d"', '"%.*f"', '"%%"');
# OFMT / CONVFMT are specified as *floating-point* conversions. "%d" is
# undefined there and every awk does something different with it (one-true-awk
# ignores it, mawk prints a garbage integer, gawk warns and prints 0), so it
# would generate noise rather than findings.
my @CONV  = ('"%.6g"', '"%.2f"', '"%.17g"', '"%.3e"');
# Width / precision supplied through `*`. Negative values are the interesting
# half — C reads a negative width as "left-justify" and a negative precision as
# "precision omitted" — so they are deliberately in range.
my @STARS = ('0', '1', '3', '6', '-1', '-4', '-8');
# `*` precision is exercised on the numeric conversions only: mawk mishandles a
# negative `*` precision on `%s` and emits megabytes of filler, which would
# swamp the harness output with one known mawk bug instead of finding new gaps.
# The `%.*s` case is pinned once, deliberately, by
# `printf_star_negative_precision_string` in probes.awkc.
# `%.*d` with a *fractional* operand that truncates to zero reaches a known gawk
# deviation: POSIX, ISO C, mawk and one-true-awk all print nothing for a zero
# value at precision 0, while gawk prints `0` when the operand was not integral.
# It stays in the vocabulary — the shape is worth generating — and is pinned by
# `printf_zero_precision_d_of_a_zero_value` in probes.awkc.
my @STAR_NUM_CONV = ('"%.*f"', '"%.*e"', '"%.*g"', '"%.*d"');
# Strings that look like numbers in one form but not another — the raw material
# for POSIX numeric-string ("strnum") questions.
my @NUMISH = ('"06"', '"6"', '"6.0"', '" 6 "', '"+6"', '"6e0"', '"0x6"', '"6abc"');
# Builtins that return a *computed* string. POSIX says a computed string is
# never a numeric string, so comparing one to a number is a string compare.
my @STRFN = ('substr(%s, 1)', 'sprintf("%%s", %s)', 'toupper(%s)', 'tolower(%s)', '(%s "")');
my @DATA  = (
    "a b c\n",
    "1 2 3\n4 5 6\n",
    "a:b:c\nd:e:f\n",
    "  leading and trailing  \n",
    "10 9\n2 10\n",
    "one\ntwo\nthree\n",
    "a,b,,c\n",
    "x\n\ny\n\n\nz\n",
    "3.14 2.72\n",
    "\n",
);

sub num  { pick(@NUMS) }
sub str  { pick(@STRS) }
sub expr {
    my $k = rnd(8);
    return num()                                  if $k == 0;
    return str()                                  if $k == 1;
    return num() . ' ' . pick('+','-','*','%') . ' ' . (num() || 1) if $k == 2;
    return '(' . num() . ' ' . pick('<','<=','>','>=','==','!=') . ' ' . num() . ')' if $k == 3;
    return str() . ' ' . str()                    if $k == 4;   # concatenation
    return 'length(' . str() . ')'                if $k == 5;
    return 'substr(' . str() . ',' . rnd(6) . ',' . rnd(6) . ')' if $k == 6;
    return '$' . rnd(4);
}

# Each template returns ($program, $stdin). Templates are the interesting part:
# they are the semantic areas, not random token soup.
my @TEMPLATES = (
    # printf / sprintf conversions
    sub {
        my $f = pick(@FMTS);
        my $arg = rnd(2) ? num() : str();
        my $extra = ($f =~ /\*/) ? (rnd(6) . ', ') : '';
        return (qq{BEGIN { printf $f "\\n", $extra$arg }}, undef);
    },
    # OFMT / CONVFMT
    sub {
        my $c = pick(@CONV);
        my $o = pick(@CONV);
        my $v = num();
        # The comparison is fully parenthesised: `print (a) == (b)` is a syntax
        # error in one-true-awk, which reads the first parenthesised group as
        # print's whole argument list.
        return (qq{BEGIN { CONVFMT = $c; OFMT = $o; x = $v; print x; print x ""; print ((x "") == ($v "")) }}, undef);
    },
    # field splitting via FS
    sub {
        my $fs = pick(@FSES);
        my $d  = pick(@DATA);
        return (qq{BEGIN { FS = $fs } { print NR, NF, "[" \$1 "]", "[" \$NF "]" }}, $d);
    },
    # OFS / ORS and \$0 rebuild
    sub {
        my $d = pick(@DATA);
        my $ofs = pick('"-"', '""', '"::"', '" "');
        my $ors = pick('"\n"', '"|"', '""');
        my $touch = rnd(2) ? '$1 = $1; ' : '';
        return (qq{BEGIN { OFS = $ofs; ORS = $ors } { $touch print; print \$1, \$2 }}, $d);
    },
    # RS forms including paragraph mode
    sub {
        my $rs = pick(@RSES);
        my $fs = pick(@FSES);
        my $d  = pick(@DATA);
        return (qq{BEGIN { RS = $rs; FS = $fs } { print NR, NF, "<" \$0 ">" } END { print "NR=" NR }}, $d);
    },
    # NF / field assignment side effects
    sub {
        my $d = pick(@DATA);
        my $k = rnd(3);
        my $body = $k == 0 ? 'NF = ' . rnd(6) . '; print NF, "[" $0 "]"'
                 : $k == 1 ? '$' . (rnd(5) + 1) . ' = "Z"; print NF, "[" $0 "]"'
                 :           '$0 = "p q r"; print NF, $2';
        return (qq{{ $body }}, $d);
    },
    # substr / index / length
    sub {
        my $s = str();
        my $m = rnd(9) - 2;
        my $l = rnd(9) - 2;
        return (qq{BEGIN { print "[" substr($s, $m, $l) "]", "[" substr($s, $m) "]", index($s, "a"), length($s) }}, undef);
    },
    # split
    sub {
        my $s  = str();
        my $fs = pick(@FSES);
        return (qq{BEGIN { n = split($s, A, $fs); print n; for (i = 1; i <= n; i++) print i, "[" A[i] "]" }}, undef);
    },
    # sub / gsub with & handling
    sub {
        my $re   = pick(@REGEX);
        my $repl = pick('"X"', '"[&]"', '"\\\\&"', '""', '"&&"');
        my $s    = str();
        my $fn   = pick('sub', 'gsub');
        return (qq{BEGIN { s = $s; n = $fn($re, $repl, s); print n, "[" s "]" }}, undef);
    },
    # match / RSTART / RLENGTH
    sub {
        my $re = pick(@REGEX);
        my $s  = str();
        return (qq{BEGIN { print match($s, $re), RSTART, RLENGTH }}, undef);
    },
    # arrays, in, delete, SUBSEP
    sub {
        my $k = rnd(3);
        my $body = $k == 0 ? 'A[1] = 1; A["1"] = 2; A[1.0] = 3; print length(A), A[1]'
                 : $k == 1 ? 'A[1,2] = "x"; print ((1,2) in A), ((2,1) in A), length(A)'
                 :           'A[1]; A[2]; A[3]; delete A[2]; print length(A), (2 in A), (1 in A)';
        return (qq{BEGIN { $body }}, undef);
    },
    # uninitialized variable semantics
    sub {
        my $op = pick('+ 0', '"" ""', '< 1', '== 0', '== ""');
        return (qq{BEGIN { print "[" x "]", (x $op), length(x) }}, undef);
    },
    # strnum comparison of fields
    sub {
        my $d = pick(@DATA);
        return (qq{{ print (\$1 == \$2), (\$1 < \$2), (\$1 "" == \$2 ""), ((\$1 + 0) == (\$2 + 0)) }}, $d);
    },
    # getline from a file the harness writes next to the case
    sub {
        my $k = rnd(2);
        my $body = $k == 0
            ? 'while ((getline line < INFILE) > 0) c++; print c; print (getline junk < INFILE)'
            : 'r = (getline < INFILE); print r, NF, "[" $0 "]", NR, FNR';
        return (qq{BEGIN { INFILE = "input.txt"; $body }}, undef);
    },
    # getline from a command pipe (fixed output, so still deterministic)
    sub {
        my $k = rnd(2);
        my $body = $k == 0
            ? 'cmd = "printf \'p\\nq\\n\'"; while ((cmd | getline l) > 0) print "G:" l; print close(cmd)'
            : 'cmd = "echo 5 6"; cmd | getline; print NF, $1, $2, NR; close(cmd)';
        return (qq{BEGIN { $body }}, undef);
    },
    # plain getline in main rules and in END
    sub {
        my $d = pick(@DATA);
        return (qq{NR == 1 { r = getline; print "r=" r, "NR=" NR, "[" \$0 "]" } END { print "eof=" getline, NR }}, $d);
    },
    # control flow and user functions
    sub {
        my $e = expr();
        return (qq{function f(a, b) { b = a * 2; return b } BEGIN { print f($e) }}, undef);
    },
    # arithmetic / comparison expressions
    sub {
        my $e = expr();
        return (qq{BEGIN { print $e }}, undef);
    },
    # exit status propagation
    sub {
        my $c = rnd(4);
        my $bare = rnd(2) ? 'exit' : "exit $c";
        return (qq{BEGIN { exit $c } END { print "end"; $bare }}, undef);
    },
    # patterns: ranges, regex, expression
    sub {
        my $d  = pick(@DATA);
        my $re = pick(@REGEX);
        my $k  = rnd(3);
        my $prog = $k == 0 ? qq{$re { print "m:" \$0 }}
                 : $k == 1 ? qq{$re, /z/ { print "r:" \$0 }}
                 :           qq{NR % 2 { print "o:" \$0 }};
        return ($prog, $d);
    },
    # `*` width and precision, including the negative forms
    sub {
        my $w = pick(@STARS);
        my $p = pick(@STARS);
        my $c = pick(@STAR_NUM_CONV);
        my $v = num();
        my $k = rnd(3);
        return (qq{BEGIN { printf "[%*d]\\n", $w, $v }}, undef)          if $k == 0;
        return (qq{BEGIN { printf "[" $c "]\\n", $p, $v }}, undef)       if $k == 1;
        return (qq{BEGIN { printf "[%*.*f]\\n", $w, $p, $v }}, undef);
    },
    # `%c` over numeric, string-literal and *field* (numeric-string) operands
    sub {
        my $d = pick("65 A 65.9\n", "97 z 0\n", "48 0 9\n");
        my $k = rnd(2);
        return (qq{BEGIN { printf "[%c][%c][%c]\\n", 65, "65", "A" }}, undef) if $k == 0;
        return (qq{{ printf "[%c][%c][%c]\\n", \$1, \$2, \$3 }}, $d);
    },
    # POSIX numeric strings: only input-derived values are strnum, so a computed
    # string compares to a number as a string. Concatenation always yields a
    # plain string, even of two numeric-looking fields.
    sub {
        my $s  = pick(@NUMISH);
        my $fn = pick(@STRFN);
        my $expr = sprintf($fn, $s);
        my $k = rnd(2);
        return (qq{BEGIN { print ($expr == 6), ($expr == "6"), ($expr < 6) }}, undef) if $k == 0;
        return (qq{{ print (\$1 == 6), (\$1 "" == 6), (\$1 \$2 == 6), (\$1 < \$2) }}, "06 6\n");
    },
    # dynamic regex held in a variable vs the same text as a literal
    sub {
        my $re = pick('"^a"', '"a.c"', '"[0-9]+"', '"\\\\."', '"c\$"', '"x*"', '"(ab)+"');
        my $s  = str();
        my $k  = rnd(3);
        return (qq{BEGIN { r = $re; print ($s ~ r), ($s !~ r) }}, undef)            if $k == 0;
        return (qq{BEGIN { r = $re; print match($s, r), RSTART, RLENGTH }}, undef)  if $k == 1;
        return (qq{BEGIN { r = $re; s = $s; print sub(r, "X", s), s }}, undef);
    },
    # split: regex literal vs string separator vs the single-space shorthand
    sub {
        my $s   = pick('"  a  b  "', '"a.b.c"', '"a1b22c"', '"a:b::c"', '"abc"');
        my $lit = pick('/ /', '/\\./', '/[0-9]+/', '/:/', '//');
        my $str = pick('" "', '"."', '"[0-9]+"', '":"', '""');
        my $k   = rnd(2);
        my $sep = $k == 0 ? $lit : $str;
        return (qq{BEGIN { n = split($s, A, $sep); print n; for (i = 1; i <= n; i++) print i, "[" A[i] "]" }}, undef);
    },
    # SUBSEP, multidimensional `in`, and CONVFMT applied to numeric subscripts
    sub {
        my $c  = pick(@CONV);
        my $ss = pick('"-"', '"\\034"', '"::"');
        my $k  = rnd(3);
        my $body = $k == 0 ? qq{CONVFMT = $c; A[1.234, 2] = 1; for (k in A) print k; print ((1.234, 2) in A)}
                 : $k == 1 ? qq{SUBSEP = $ss; A[1, 2] = "v"; print ((1, 2) in A), ((2, 1) in A), length(A); delete A[1, 2]; print length(A)}
                 :           qq{A[1, 2] = 1; A[1, 3] = 2; delete A; print length(A), ((1, 2) in A)};
        return (qq{BEGIN { $body }}, undef);
    },
    # `delete arr` on a whole array, including through a function parameter
    sub {
        my $k = rnd(2);
        return (qq{function clear(a) { delete a; return length(a) } BEGIN { A[1] = 1; A[2] = 2; print clear(A), length(A) }}, undef) if $k == 0;
        return (qq{BEGIN { A[1] = 1; delete A; print length(A), (1 in A); delete B; print length(B) }}, undef);
    },
    # print redirection: overwrite vs append, and the close() return value
    sub {
        my $k = rnd(2);
        my $body = $k == 0
            ? q{print "a" > "f"; print "b" > "f"; close("f"); while ((getline l < "f") > 0) print "W:" l}
            : q{print "a" > "f"; close("f"); print "b" >> "f"; close("f"); while ((getline l < "f") > 0) print "A:" l};
        return (qq{BEGIN { $body; print (close("never-opened") < 0) }}, undef);
    },
    # system() must flush pending stdout before the child runs
    sub {
        my $c = rnd(4);
        return (qq{BEGIN { print "b"; system("echo m"); printf "n"; system(""); print "a"; print system("exit $c") }}, undef);
    },
    # srand() returns the *previous* seed
    sub {
        my ($a, $b) = (rnd(100), rnd(100));
        return (qq{BEGIN { srand($a); print srand($b), srand($a) }}, undef);
    },
    # empty statement as a control-flow body (POSIX `terminated_statement`)
    sub {
        my $k = rnd(3);
        my $body = $k == 0 ? 'if (1) ; print "A"'
                 : $k == 1 ? 'for (i = 0; i < 2; i++) ; print i'
                 :           'if (0) ; else print "C"';
        return (qq{BEGIN { $body }}, undef);
    },
    # `break` / `continue` in every loop form. Until these templates existed the
    # whole corpus — 77 curated probes and 32 templates — contained not one
    # `continue` and not one `break` outside the boilerplate prelude of the
    # parity cases, so no generated program could ever have exercised the
    # per-loop-form patch lists the compiler keeps for them.
    sub {
        my $n = rnd(4) + 2;
        my $hit = rnd($n);
        my $jump = pick('break', 'continue');
        my $k = rnd(4);
        my $body = "if (i == $hit) $jump; print i";
        return (qq{BEGIN { for (i = 0; i < $n; i++) { $body } print "after", i }}, undef) if $k == 0;
        return (qq{BEGIN { i = 0; while (i < $n) { i++; $body } print "after", i }}, undef) if $k == 1;
        return (qq{BEGIN { i = 0; do { i++; $body } while (i < $n); print "after", i }}, undef) if $k == 2;
        # `for (k in a)` needs a body that does not depend on the iteration
        # order, which awk leaves unspecified — mawk and awkrs really do visit
        # a two-element array in opposite orders. Branching on the *visit
        # counter* rather than on the key keeps the result well-defined.
        return (qq{BEGIN { for (j = 0; j < $n; j++) a[j] = j; c = 0; for (k in a) { c++; if (c == 2) $jump; c += 10 } print c }}, undef);
    },
    # `break` / `continue` two loop forms deep, and inside a function body —
    # the shape where a signal escaping the wrong chunk shows up.
    sub {
        my $jump = pick('break', 'continue');
        my $k = rnd(3);
        return (qq{BEGIN { for (i = 0; i < 3; i++) { for (j = 0; j < 3; j++) { if (j == 1) $jump; print i, j } } }}, undef) if $k == 0;
        return (qq{BEGIN { i = 0; while (i < 3) { i++; j = 0; do { j++; if (j == 2) $jump; if (j > 4) break; print i, j } while (j < 4) } }}, undef) if $k == 1;
        return (qq{function f(n,   i, s) { for (i = 0; i < n; i++) { if (i == 2) $jump; s = s i } return s } BEGIN { print "[" f(5) "]" }}, undef);
    },
    # `next` / `nextfile` from a rule, including from inside a loop, plus the
    # `END` block that still has to run afterwards.
    sub {
        my $d = pick(@DATA);
        my $k = rnd(4);
        return (qq{NR == 2 { next } { print NR, "[" \$0 "]" } END { print "END", NR }}, $d)                     if $k == 0;
        return (qq{{ for (i = 0; i < 3; i++) if (i == 1) next; print "kept", NR } END { print "END", NR }}, $d) if $k == 1;
        return (qq{{ i = 0; while (i < 3) { i++; if (i == 2) next }; print "kept", NR } END { print "END", NR }}, $d) if $k == 2;
        return (qq{{ nextfile } END { print "END", NR, FNR }}, $d);
    },
    # `exit` with and without a status, from every block, including from a
    # function and from inside a loop. `END` still runs, and a bare `exit` there
    # preserves the status the first `exit` set.
    sub {
        my $c = rnd(4);
        my $k = rnd(5);
        return (qq{BEGIN { for (i = 0; i < 5; i++) if (i == 2) exit $c; print "no" } END { print "end" }}, undef) if $k == 0;
        return (qq{function f(n) { if (n == 2) exit $c; return n } BEGIN { for (i = 0; i < 5; i++) f(i); print "no" } END { print "end" }}, undef) if $k == 1;
        return (qq{{ print NR; if (NR == 2) exit $c } END { print "end" }}, "a\nb\nc\n")                        if $k == 2;
        return (qq{{ exit $c } END { print "end"; exit }}, "a\nb\n")                                            if $k == 3;
        return (qq{END { i = 0; do { i++; if (i == 2) exit $c } while (i < 5); print "no" }}, "a\n");
    },
    # Arithmetic builtins and the operators that had no coverage at all:
    # int/sqrt/exp/log/sin/cos/atan2, `^`, unary minus on a string, `?:`,
    # pre/post increment, and the compound assignments.
    sub {
        my $v = num();
        my $k = rnd(5);
        return (qq{BEGIN { print int($v), int(-($v)), int("12abc"), int("") }}, undef)                       if $k == 0;
        # sqrt/log of a negative are NaN / -inf and every awk spells those
        # differently, so the arguments are kept in the defined domain.
        return (qq{BEGIN { x = ($v < 0 ? -($v) : $v); print sqrt(x), exp(0), log(1), atan2(0, -1) }}, undef) if $k == 1;
        return (qq{BEGIN { print sin(0), cos(0), 2 ^ 10, 2 ^ 0.5, (-2) ^ 3 }}, undef)                        if $k == 2;
        return (qq{BEGIN { x = $v; print x++, x, ++x, x--, x, --x }}, undef)                                 if $k == 3;
        return (qq{BEGIN { x = 10; x += 3; print x; x -= 2; print x; x *= 2; print x; x /= 4; print x; x %= 3; print x; x ^= 3; print x }}, undef);
    },
    # Short-circuit `&&` / `||` must not evaluate the right operand, and `!`
    # applies awk's own truthiness (a non-empty *string* "0" is true; a numeric
    # string "0" from input is false).
    sub {
        my $d = pick("0 0\n", "1 x\n", "00 a\n");
        my $k = rnd(2);
        # The side effect is a counter, not a `print`: one-true-awk orders the
        # output of a `print` whose arguments themselves print differently from
        # gawk and mawk, which would report that ordering difference on every
        # case instead of the short-circuit behaviour this is here to check.
        return (qq{function f() { n++; return 1 } BEGIN { r1 = (0 && f()); r2 = (1 || f()); r3 = (1 && f()); r4 = (0 || f()); print r1, r2, r3, r4, n }}, undef) if $k == 0;
        return (qq{{ print !\$1, !\$2, !"0", !"", !0, !1 }}, $d);
    },
    # A field or `$0` **assigned** a plain string stops being a POSIX numeric
    # string, so a later relational compares as text. Assigning a number or
    # another field keeps it numeric. Splitting always yields numeric strings.
    sub {
        my $s = pick(@NUMISH);
        my $k = rnd(4);
        return (qq{{ \$0 = $s; print (\$0 == 6), (\$0 < 7), (\$1 == 6) }}, "42\n")     if $k == 0;
        return (qq{{ \$1 = $s; print (\$1 == 6), (\$1 < 7) }}, "42 7\n")               if $k == 1;
        return (qq{{ \$1 = \$2; print (\$1 < 7), (\$1 == \$2) }}, "42 6\n")            if $k == 2;
        return (qq{BEGIN { \$0 = $s; print (\$0 == 6), (\$0 < 7), NF, "[" \$1 "]" }}, undef);
    },
    # Every op that takes a **single, non-integral** subscript, under a CONVFMT
    # that does not round-trip it. Until this template existed the corpus could
    # not reach the bug it finds: each generated `in`/`delete` used either an
    # integral subscript — which bypasses CONVFMT entirely, so any conversion
    # round-trips — or the multidimensional form, where the SUBSEP join has
    # already reduced the key to a string before the op runs. `in`, `delete`,
    # `a[k] += v`, `a[k]++` and a plain read each converted the key their own
    # way, so `a[x] = 1; (x in a)` was false and `a[x] += 1` created a *second*
    # entry instead of updating the first.
    sub {
        my $c = pick(@CONV);
        my $v = pick('1.23456', '2 / 3', '0.1 + 0.2', '1 / 3', '3.14159265', '1e-3 + 1e-4');
        my $k = rnd(4);
        return (qq{BEGIN { CONVFMT = $c; x = $v; A[x] = 1; print (x in A), length(A); delete A[x]; print length(A), (x in A) }}, undef)      if $k == 0;
        return (qq{BEGIN { CONVFMT = $c; x = $v; A[x] = 5; A[x] += 2; print length(A), A[x]; for (j in A) print "[" j "]" }}, undef)         if $k == 1;
        return (qq{BEGIN { CONVFMT = $c; x = $v; A[x] = 5; A[x]++; --A[x]; print length(A), A[x] }}, undef)                                  if $k == 2;
        return (qq{BEGIN { CONVFMT = $c; x = $v; A[x] = "v"; print (x in A), ((x "") in A), A[x], length(A) }}, undef);
    },
    # RS as a regex / multi-character literal, and the empty-record field count
    sub {
        my $rs = pick('"[0-9]+"', '"ab"', '":"', '"\\n\\n+"');
        my $fs = pick('" "', '":"', '"[0-9]+"', '"\\t"');
        my $d  = pick("a1b22c", "XabYabZ\n", "a:b:\n", "\na:b\n\n", "\n\n");
        return (qq{BEGIN { RS = $rs; FS = $fs } { print NR, NF, "<" \$0 ">" } END { print "NR=" NR }}, $d);
    },
);

my $fh;
if (defined $out) { open $fh, '>', $out or die "gen_awk.pl: $out: $!\n" } else { $fh = \*STDOUT }

for my $i (1 .. $n) {
    my $t = $TEMPLATES[rnd(scalar @TEMPLATES)];
    my ($prog, $in) = $t->();
    printf {$fh} "#=== gen%05d\n", $i;
    print {$fh} "#--- prog\n", $prog, "\n";
    if (defined $in) {
        # The corpus reader only recognises `#=== name` at the start of a line,
        # so a stdin block that does not end in a newline glues the *next*
        # case's header onto its last record: both cases are then read as one
        # and the second one silently disappears from the run. `RS = "[0-9]+"`
        # data like "a1b22c" did exactly that, so three of every four hundred
        # generated cases were being lost. The format cannot express a final
        # record without a terminator anyway — that is what the curated probes
        # are for — so the newline is added here rather than trusted to each
        # template.
        $in .= "\n" unless $in =~ /\n\z/;
        print {$fh} "#--- in\n", $in;
    }
}
close $fh if defined $out;
