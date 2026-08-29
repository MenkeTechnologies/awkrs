# `split` numbers its results from 1, and those subscripts behave as integer
# subscripts everywhere after: `in`, `length`, and the index sorts. `asorti`
# renders each subscript back to a string in the destination.
BEGIN {
    PROCINFO["sorted_in"] = "@ind_num_asc"
    n = split("50 3 9 1", arr)
    print n, length(arr), (1 in arr), ("1" in arr), (0 in arr)
    for (i in arr) printf "  %s:%s\n", i, arr[i]
    m = asort(arr, sorted)
    print "asort:", m
    for (i in sorted) printf "  v%s=%s\n", i, sorted[i]
    k = asorti(arr, idx)
    print "asorti:", k
    for (i in idx) printf "  i%s=%s\n", i, idx[i]
}
