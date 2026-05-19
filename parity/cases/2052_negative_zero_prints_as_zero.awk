# gawk parity: -0.0 is printed as "0", not "-0".
BEGIN {
    print -0.0
    print 0.0 - 0
    print -1 * 0
    print 5 - 5

    # Through OFMT / CONVFMT paths too.
    OFMT  = "%.6g"
    CONVFMT = "%.6g"
    x = -0.0
    print x
    s = x ""
    print "[" s "]"
}
