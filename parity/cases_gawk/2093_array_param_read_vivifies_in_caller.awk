# Reading a missing subscript CREATES it, and creates it in the array that was
# passed in — not under the parameter's name in the globals. `p` must stay
# empty after the call while the caller's array grows.
#
# The vivified element is probed by VALUE, not by typeof(): gawk renamed that
# spelling mid-series — 5.2.1 reports `untyped` for a vivified element and
# 5.4.1 reports `unassigned` — so a typeof() column here pins the corpus to
# whichever gawk recorded it (it failed CI on 5.2.1 after being written on
# 5.4.1). The dual-typed value is the semantics vivification actually creates,
# and it reads identically on every gawk. typeof() still covers the ASSIGNED
# element, whose `number` has been stable since typeof() landed in 4.2.
function probe(p,   unused) { unused = p["missing"] }
BEGIN {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    q["a"] = 1
    probe(q)
    print ("missing" in p), length(p)
    print ("missing" in q), length(q)
    print "  assigned type=" typeof(q["a"])
    for (k in q) print "  q[" k "]=" q[k] " empty=" (q[k] == "") " zero=" (q[k] == 0)
}
