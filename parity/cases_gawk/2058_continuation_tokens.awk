# POSIX / gawk: certain tokens at line end act as statement continuation —
# the following newline is whitespace, not a terminator. Awkrs now matches
# gawk on `,`, `||`, `&&`, `?`, and `:`.
function f(a, b, c) { return a + b + c }
BEGIN {
    # Comma continuation in print, printf, function call.
    print "a",
          "b",
          "c"
    printf "%s|%s|%s\n",
        "p",
        "q",
        "r"
    print f(1,
            2,
            3)

    # Logical operator continuation.
    print (0 ||
           0 ||
           1)
    print (1 &&
           1 &&
           1)

    # Ternary continuation.
    print (1 ?
           "yes" :
           "no")
}
