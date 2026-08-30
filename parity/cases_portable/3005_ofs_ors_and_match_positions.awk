
# portable:3005 — OFS applies when $0 is REBUILT, ORS terminates `print`, and
# `match` reports its position through RSTART/RLENGTH including on failure.
BEGIN {
    OFS = "-"
    $0 = "a b c"
    printf "[%s]\n", $0
    $1 = $1
    printf "[%s]\n", $0
    ORS = "|"
    print "x", "y", "z"
    ORS = "\n"
    print ""
    printf "%d %d %d\n", match("hello world", /o w/), RSTART, RLENGTH
    printf "%d %d %d\n", match("hello", /zz/), RSTART, RLENGTH
    printf "%d %d %d\n", match("abc", //), RSTART, RLENGTH
    # An uninitialized variable is both "" and 0 and compares equal to each.
    printf "[%s] %d %d %d\n", u, u, (u == 0), (u == "")
    # Reading a field past NF does not extend the record; assigning one does,
    # padding with empty fields joined by OFS.
    $0 = "p q"
    printf "%d [%s] %d\n", NF, $5, NF
    $5 = "e"
    printf "%d [%s]\n", NF, $0
    # SUBSEP joins a multi-dimensional subscript, and (i, j) in a tests it.
    arr[1, 2] = "v"
    for (k in arr) { n = split(k, parts, SUBSEP); printf "%d %s %s\n", n, parts[1], parts[2] }
    printf "%d %d\n", ((1, 2) in arr), ((1, 3) in arr)
}
