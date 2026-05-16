function clear(a,    k) { for (k in a) delete a[k] }
BEGIN {
    x["a"] = 1
    x["b"] = 2
    clear(x)
    print length(x)
}
