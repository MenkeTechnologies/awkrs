BEGIN {
    t = systime()
    s = strftime("%Y", t)
    # Year should be 4 digits and >= 2024
    print (length(s) == 4 && s+0 >= 2024 ? "ok" : "fail")
    # Day of week
    d = strftime("%A", 0)
    print (length(d) > 0 ? "ok" : "fail")
    # Fixed format
    print strftime("%H:%M", mktime("2024 1 1 13 45 0"))
}
