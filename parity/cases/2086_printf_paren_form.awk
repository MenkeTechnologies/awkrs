# gawk parity: `printf("fmt", a, b)` is the function-call form of `printf "fmt", a, b`.
# Earlier awkrs rejected the parenthesized form with a "parenthesized comma list"
# parse error. Both forms must produce identical output.
BEGIN {
    printf("%d %d\n", 1, 2)
    printf("hello\n")
    printf("%d %s %d\n", 1, "two", 3)
    # Single-arg paren form still works.
    printf("just text\n")
    # Format from a variable, no arg list.
    fmt = "%d-%d\n"
    printf(fmt, 7, 8)
    # Sprintf with paren form (always was supported via function-call grammar).
    s = sprintf("(%d,%d)", 5, 6)
    print s
}
