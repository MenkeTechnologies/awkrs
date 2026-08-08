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
);

my $fh;
if (defined $out) { open $fh, '>', $out or die "gen_awk.pl: $out: $!\n" } else { $fh = \*STDOUT }

for my $i (1 .. $n) {
    my $t = $TEMPLATES[rnd(scalar @TEMPLATES)];
    my ($prog, $in) = $t->();
    printf {$fh} "#=== gen%05d\n", $i;
    print {$fh} "#--- prog\n", $prog, "\n";
    if (defined $in) { print {$fh} "#--- in\n", $in }
}
close $fh if defined $out;
