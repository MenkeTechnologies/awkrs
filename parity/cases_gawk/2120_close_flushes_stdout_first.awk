# gawk's do_close flushes stdout before closing an open redirection, so text
# the program printed earlier lands ahead of output the closing command
# still has to write.
BEGIN {
    printf "x" | "cat"; printf "y"; close("cat"); print ""
    print "1"; print "b" | "sort"; print "2"; close("sort"); print "3"
}
