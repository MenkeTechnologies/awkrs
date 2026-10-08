# portable:3011 — POSIX `getline lvalue`: the target may be an array element
# or a field, not only a plain name, from the main input, a file and a command.
NR == 1 {
    getline a[1]
    print "elem:", a[1], "NR=" NR
    i = 2
    getline a[i, "k"]
    print "multi:", a[2, "k"]
    getline $2
    print "field:", $0, "NF=" NF, "$2=" $2
    "echo piped" | getline cmd["out"]
    print "cmd:", cmd["out"]
    close("echo piped")
    "echo f3" | getline $(1 + 2)
    print "field3:", $0, "NF=" NF
    while ((getline rest[++n]) > 0)
        ;
    print "attempts:", n, "last:", rest[n - 1]
    if ((getline x[1] < "/nonexistent/file") < 0)
        print "missing file: -1"
}
