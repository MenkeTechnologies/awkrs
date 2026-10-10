# gawk:2131 — `getline lvalue` evaluates the lvalue's subscript before the read
# is known to succeed, so `rest[++n]` increments `n` on the attempt that hits
# end of input too. one-true-awk (2023 releases) evaluates the lvalue only on a
# successful read and counts one fewer, which is why this is not a portable case.
NR == 1 {
    while ((getline rest[++n]) > 0)
        ;
    print "attempts:", n, "last:", rest[n - 1]
}
