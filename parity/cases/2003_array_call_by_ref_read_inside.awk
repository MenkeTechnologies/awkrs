function f(a) {
    if (a["k"] == "v") print "found"
    else print "missing"
}
BEGIN {
    x["k"] = "v"
    f(x)
}
