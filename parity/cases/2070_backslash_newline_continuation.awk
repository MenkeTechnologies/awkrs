# POSIX / gawk: a backslash immediately followed by a newline is line
# continuation — the next physical line continues the current statement.
BEGIN {
    x = "line1\n" \
        "line2\n" \
        "line3"
    print x

    # Continuation inside a function call's argument list.
    printf "%d %d %d\n", \
        1, \
        2, \
        3

    # Multiple statements per "logical line" via continuation.
    y = 1; z = 2; \
    sum = y + z; \
    print sum
}
