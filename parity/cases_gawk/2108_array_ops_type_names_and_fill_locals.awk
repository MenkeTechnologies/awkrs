# gawk: `in`, `for (k in …)`, `delete a[k]` and an rvalue `a[k]` type an
# unassigned name as an array, for globals and function locals alike; and
# patsplit/match/split fill a function's local or parameter array, not a
# global of the same name.
function in_local(   l) { x = (1 in l); return typeof(l) }
function for_local(   l, k) { for (k in l); return typeof(l) }
function del_local(   l) { delete l[1]; return typeof(l) }
function read_local(   l) { l[1]; return typeof(l) length(l) }
function pats(s,   l, seps) { patsplit(s, l, /[a-z]+/, seps); return length(l) "/" l[2] "/" seps[1] }
function m(s,   l) { match(s, /(b+)(c)/, l); return l[1] "/" l[2, "start"] }
function sp(s, arr, seps) { return split(s, arr, /-+/, seps) }
BEGIN {
    if (1 in g1); print typeof(g1), isarray(g1)
    for (k in g2); print typeof(g2)
    delete g3[1]; print typeof(g3)
    print in_local(), for_local(), del_local(), read_local()
    print typeof(l)
    print pats("ab  cd,ef"), typeof(seps)
    print m("abbbc"), typeof(l)
    print sp("a--b-c", A, S), A[3], S[1], length(S)
}
