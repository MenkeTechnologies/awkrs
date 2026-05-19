# gawk parity: a user-function call passes its arguments by *reference* when
# the called function uses the parameter as an array. The caller's variable
# becomes an array even if it never appears in an index expression at the
# call site — the array-ness propagates from the function body.
function fill(a) {
    a[1] = 10
    a[2] = 20
    a[3] = 30
}

function fill_rec(a, n) {
    if (n > 0) {
        a[n] = n * 10
        fill_rec(a, n - 1)
    }
}

BEGIN {
    fill(direct)
    print length(direct), direct[1], direct[2], direct[3]

    fill_rec(rec, 4)
    print length(rec)
    for (i = 1; i <= 4; i++) printf "%d=%d ", i, rec[i]
    print ""
}
