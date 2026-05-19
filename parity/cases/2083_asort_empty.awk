# gawk parity: `asort(a)` / `asorti(a)` on a name that has never been assigned
# treats it as an empty array — returns 0 — NOT a "first argument is not an
# array" fatal. Pure scalars (Num/Str) still fatal at runtime.
BEGIN {
    n = asort(empty1)
    print "empty asort:", n
    m = asorti(empty2, dest)
    print "empty asorti:", m, length(dest)

    # Normal sort still works.
    a[1]=3; a[2]=1; a[3]=2
    nn = asort(a)
    print "sorted:", nn, a[1], a[2], a[3]
}
