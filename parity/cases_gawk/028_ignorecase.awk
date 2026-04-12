BEGIN {
    # IGNORECASE affects match
    IGNORECASE = 1

    print match("Hello World", /hello/)
    print (RSTART > 0 ? "ok" : "fail")

    # case-insensitive string comparison
    print ("abc" == "ABC" ? "eq" : "ne")

    IGNORECASE = 0
    print match("Hello", /hello/)
}
