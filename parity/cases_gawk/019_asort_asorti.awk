BEGIN {
    # asort: sort array values
    a[1] = "cherry"
    a[2] = "apple"
    a[3] = "banana"
    n = asort(a)
    for (i = 1; i <= n; i++) print a[i]

    # asorti: sort array indices
    delete b
    b["z"] = 1; b["a"] = 2; b["m"] = 3
    n = asorti(b, c)
    for (i = 1; i <= n; i++) print c[i]
}
