BEGIN {
    # systime returns current epoch seconds (should be > 0)
    t = systime()
    print (t > 1000000000 ? "ok" : "fail")

    # strftime with systime
    s = strftime("%Y", t)
    print (length(s) == 4 ? "ok" : "fail")
}
