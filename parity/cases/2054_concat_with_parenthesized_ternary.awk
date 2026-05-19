# Regression: `"a" (cond ? "b" : "c")` must concat the OUTER string with the
# selected ternary branch — gawk produces "ab" (true) or "ac" (false), never
# "bc". A faulty peephole used to fuse the trailing Concat with the ELSE PushStr.
BEGIN {
    print "a" (1 ? "b" : "c")
    print "a" (0 ? "b" : "c")
    x = "head-" (1 ? "TRUE" : "FALSE") "-tail"; print x
    y = "head-" (0 ? "TRUE" : "FALSE") "-tail"; print y

    # Multiple ternaries in a row.
    print (1 ? "p" : "q") "_" (0 ? "r" : "s")
}
