function f(a) { a["new"] = 99 }
BEGIN {
    x["old"] = 1
    f(x)
    print x["old"]
    print x["new"]
}
