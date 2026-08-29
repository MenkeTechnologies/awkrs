# Reading a missing subscript CREATES it, and creates it in the array that was
# passed in — not under the parameter's name in the globals. `p` must stay
# empty after the call while the caller's array grows.
function probe(p,   unused) { unused = p["missing"] }
BEGIN {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    q["a"] = 1
    probe(q)
    print ("missing" in p), length(p)
    print ("missing" in q), length(q)
    for (k in q) print "  q[" k "]=" q[k] " type=" typeof(q[k])
}
