# `cmd | getline x` keeps the subprocess open between calls; successive reads
# advance through the same stdout stream.
BEGIN {
    cmd = "printf '%s\\n' one two three"
    while ((cmd | getline line) > 0) print "got:", line
    close(cmd)
    print "done"
}
