# Negative zero subscripts the SAME element as zero — zero renders as "0", and
# the subscript is that rendering. A negative integer keeps its sign.
BEGIN {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    a[-0] = 1; a[0] = 2
    print length(a), a[0], a[-0]
    for (k in a) print "zero key=[" k "]"
    b[-5] = "neg"
    print (-5 in b), ("-5" in b), b["-5"]
    for (k in b) print "neg key=[" k "]"
}
