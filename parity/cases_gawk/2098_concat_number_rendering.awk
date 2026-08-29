# Appending a number to a string renders it the way a subscript does: an
# integral value in integer form, never through CONVFMT, either side of the
# 32-bit range and at the 2^53 boundary. A non-integral value does go through
# CONVFMT.
BEGIN {
    s = ""
    s = s -1;  s = s " "; s = s 0;   s = s " "; s = s 2147483647
    s = s " "; s = s -2147483648;    s = s " "; s = s 1e15
    s = s " "; s = s 1e16
    print s
    t = ""; t = t 9007199254740992; t = t "|"; t = t -9007199254740992
    print t
    CONVFMT = "%.2f"
    u = ""; u = u 3.14159; u = u " "; u = u 100
    print u
    v = 5; v = v 6
    print v, v + 1, length(v)
}
