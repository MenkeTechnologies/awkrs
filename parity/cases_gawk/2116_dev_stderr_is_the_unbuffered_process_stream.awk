# gawk writes `/dev/stderr` (and `/dev/fd/2`) through the process's own
# unbuffered stderr and `/dev/fd/1` through the buffered stdout, so stderr
# lines land at once, in program order, while stdout waits for a flush.
# close() of such a name answers 0 once a print opened it and -1 otherwise.
BEGIN {
    printf "x" > "/dev/stderr"
    print "o"
    fflush()
    printf "y\n" > "/dev/stderr"
    print "1" > "/dev/fd/2"; print "2" > "/dev/stderr"; print "3" > "/dev/fd/2"
    print "a" > "/dev/fd/1"
    print "b" > "/dev/fd/2"
    print "c"
    r1 = close("/dev/stderr"); r2 = close("/dev/stderr"); r3 = close("/dev/stdout")
    r4 = close("/dev/fd/1"); r5 = fflush("/dev/stderr"); r6 = fflush("/dev/stdout")
    print r1, r2, r3, r4, r5, r6
}
