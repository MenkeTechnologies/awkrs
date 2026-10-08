# gawk:2126 — `nextfile` in BEGINFILE skips the file unread and the rest of
# the BEGINFILE rules; ENDFILE still runs for it unless it failed to open, so
# `BEGINFILE { if (ERRNO != "") nextfile }` steps over a missing operand.
BEGIN {
    ARGV[2] = "/nonexistent/awkrs-parity-2126"
    ARGV[3] = ARGV[1]
    ARGC = 4
}
BEGINFILE {
    n++
    print "BEGINFILE", n, (ERRNO != "" ? "open failed" : "open ok"), FNR
    if (ERRNO != "" || n == 1)
        nextfile
    print "reading", n
}
BEGINFILE { print "second BEGINFILE rule", n }
{ print n, FNR, $0 }
ENDFILE { print "ENDFILE", n, FNR }
END { print "NR", NR }
