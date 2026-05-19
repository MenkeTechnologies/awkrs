# gawk: IGNORECASE applies to multi-char regex FS, but NOT to a single-char
# string FS or a single-char split() separator.
BEGIN {
    IGNORECASE = 1

    # Multi-char FS: IGNORECASE applies.
    n = split("aXXbYYc", a, "[xy]+")
    print "regex:", n, a[1], a[2], a[3]

    # Single-char string FS: literal, NEVER case-insensitive.
    m = split("aXbXc", b, "x")
    print "literal:", m, "[" b[1] "]"
}
