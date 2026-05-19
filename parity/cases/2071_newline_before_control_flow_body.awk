# POSIX / gawk: a newline between a control-flow head and its single-statement
# body is whitespace. Allows the K&R style `if (cond)\n  body` formatting.
BEGIN {
    if (1)
        print "if-yes"
    if (0)
        print "no"
    else
        print "else-yes"

    for (i = 0; i < 3; i++)
        print "for", i

    j = 0
    while (j < 3)
        j++
    print "after-while", j

    k = 0
    do
        k++
    while (k < 2)
    print "after-do", k
}
