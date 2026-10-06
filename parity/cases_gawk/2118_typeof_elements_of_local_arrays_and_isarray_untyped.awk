# typeof(a[k]) looks the element up where the array lives: an array
# parameter or a local array is found in the function's frame. The subscript
# makes an untyped name an array (gawk Op_subscript), and isarray() leaves an
# untyped variable untyped (Op_push_arg_untyped).
function f(a,    l) {
    a[1] = 1; a[2]
    l["s"] = "str"
    print typeof(a[1]), typeof(a[2]), typeof(a[3]), typeof(l["s"]), typeof(l["t"])
}
function g(p) { print typeof(p[1]); print typeof(p) }
BEGIN {
    f(arr)
    g(q); print typeof(q)
    print typeof(u[1]); print typeof(u)
    print isarray(v); print typeof(v)
    w = 1; print isarray(w), typeof(w)
}
