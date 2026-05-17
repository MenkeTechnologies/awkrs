BEGIN {
    x = a["k"]
    if ("k" in a) print "auto-created"
    else print "not created"
    print typeof(a["k"])
}
