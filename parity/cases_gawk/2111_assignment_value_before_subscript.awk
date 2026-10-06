# gawk computes an assignment's value before the subscript or field it is
# stored under (mawk and one-true-awk differ, and disagree with each other).
function bump() { n = 5; return "v" }
function rewrite() { $0 = "p q r s"; return "Z" }
BEGIN {
    i = 1; a[i++] = i; print length(a), (1 in a), a[1]
    k = 1; b[k++] += k; print (1 in b), b[1], (2 in b)
    m = 1; c[m] = m++; print (1 in c), (2 in c), c[2]
    q = 1; e[q, q++] = q; print ((1, 1) in e), e[1, 1]
    n = 1; f[n] = bump(); print (1 in f), f[5]
    j = 1; $0 = "x y"; $(j++) = j; print $0, NF
    $0 = "x y"; $NF = rewrite(); print $0
    i = 1; $0 = "5 6 7"; $(i++) += i; print $0
}
