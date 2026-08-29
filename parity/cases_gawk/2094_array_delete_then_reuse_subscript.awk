# A deleted subscript can be created again and reads back as what was stored,
# not as anything the first life left behind. `delete a` empties the whole
# array without disturbing later use.
BEGIN {
    PROCINFO["sorted_in"] = "@ind_num_asc"
    for (i = 1; i <= 4; i++) a[i] = i * 10
    delete a[2]
    print length(a), (2 in a), a[2], length(a)
    a[2] = 99
    print length(a), a[2]
    for (k in a) printf "  %s=%s\n", k, a[k]
    delete a
    print "emptied:", length(a), (1 in a)
    a[7] = "seven"
    print length(a), a[7]
}
