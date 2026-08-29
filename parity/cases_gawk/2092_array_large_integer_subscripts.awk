# Subscripts either side of 2^53, where a double stops representing every
# integer, and one past the signed 64-bit range. Each renders exactly as the
# number prints, so the element it names follows the same rule as any other.
BEGIN {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    a[9007199254740992] = "two53"
    a[9007199254740993] = "two53p1"
    print length(a)
    for (k in a) print "  [" k "]=" a[k]
    b[2147483648] = 1; b[-2147483649] = 2
    print length(b)
    for (k in b) print "  32bit [" k "]"
}
