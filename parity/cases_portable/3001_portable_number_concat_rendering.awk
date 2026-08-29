# portable:3001 — appending a number to a string renders an integral value in
# integer form, never through CONVFMT, across the 32-bit boundaries; a
# non-integral value renders through CONVFMT.
BEGIN {
    s = ""
    s = s -1;  s = s " "; s = s 0
    s = s " "; s = s 2147483647
    s = s " "; s = s -2147483648
    print s
    CONVFMT = "%.2f"
    t = ""; t = t 3.14159; t = t " "; t = t 100
    print t
    u = 5; u = u 6
    printf "%s %d %d\n", u, u + 1, length(u)
    v = ""; v = v 0; v = v -0
    printf "[%s] %d\n", v, length(v)
}
