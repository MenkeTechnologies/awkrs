function f(s) { s = "modified" }
BEGIN {
    x = "orig"
    f(x)
    print x
}
