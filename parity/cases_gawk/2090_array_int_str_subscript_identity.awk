# `a[1]` and `a["1"]` are ONE element: a numeric subscript renders through the
# same rule a string subscript is compared by. The spellings that are NOT that
# rendering — a leading zero, a sign, surrounding space, a trailing `.0` — each
# name their own element.
BEGIN {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    a[1] = "int"; a["1"] = "str"
    print length(a), a[1], a["1"]
    b[1.0] = "x"
    print (1 in b), ("1" in b), length(b)
    c["01"] = 1; c[1] = 2;    print "01:", length(c)
    d["+1"] = 1; d[1] = 2;    print "+1:", length(d)
    e[" 1"] = 1; e[1] = 2;    print "sp:", length(e)
    f["1.0"] = 1; f[1] = 2;   print "1.0:", length(f)
    for (k in f) print "  key=[" k "]"
}
