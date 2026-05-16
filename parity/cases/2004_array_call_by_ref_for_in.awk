function f(a,    k, n) {
    n = 0
    for (k in a) n++
    return n
}
BEGIN {
    x["a"] = 1
    x["b"] = 2
    x["c"] = 3
    print f(x)
}
