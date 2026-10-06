
# portable:3008 — split() into a function's array parameter or local, and the
# scope of a function's local arrays.
function fill(s, arr) { return split(s, arr, ",") }
function first(s,   parts) { split(s, parts, ","); return parts[1] }
function count(s,   parts, n) { n = split(s, parts, ","); return n ":" length(parts) }
function sees_global(   k, r) { for (k in shadow) r = r k; return r }
function shadows(   shadow) { shadow["local"] = 1; return sees_global() }
function writes_global() { leak["g"] = 1 }
function local_leak(   leak) { writes_global(); return length(leak) }
function member(k,   loc) { loc["a"]; return (k in loc) }
BEGIN {
    n = fill("x,y,z", A)
    print n, A[1], A[3], length(A)
    print first("p,q"), first("r,s,t"), length(parts)
    print count("1,2,3,4"), count("5")
    shadow["global"] = 1
    print shadows()
    print local_leak(), length(leak)
    print member("a"), member("b"), length(loc)
}
