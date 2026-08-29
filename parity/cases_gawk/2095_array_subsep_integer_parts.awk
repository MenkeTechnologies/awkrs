# A multi-dimensional subscript is its parts joined by SUBSEP, so an integer
# part renders the same way a lone integer subscript does and the two spellings
# name one element.
BEGIN {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    SUBSEP = ","
    a[1, 2] = "num"
    a["1", "2"] = "str"
    print length(a), a[1, 2]
    print ((1, 2) in a), (("1", "2") in a)
    b[0, -0] = "zeros"
    for (k in b) { n = split(k, part, SUBSEP); print "  parts:", n, part[1], part[2] }
}
