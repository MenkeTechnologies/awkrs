BEGIN {
    s = "abc"
    n = gsub(//, "-", s)
    print n, s

    t = ""
    m = gsub(//, "X", t)
    print m, "[" t "]"

    u = "hi"
    k = gsub("", "*", u)
    print k, u
}
