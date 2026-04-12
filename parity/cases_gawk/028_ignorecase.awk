BEGIN {
    # IGNORECASE affects match
    IGNORECASE = 1

    print match("Hello World", /hello/)
    print (RSTART > 0 ? "ok" : "fail")

    n = split("aAbBcC", arr, /a/)
    print n

    IGNORECASE = 0
    print match("Hello", /hello/)
}
