# gawk sets ARGV[0] to the binary's basename, not the full path.
# Just check that the value contains no path separators so the parity diff is stable
# regardless of where each awk lives on disk.
BEGIN {
    if (index(ARGV[0], "/") == 0) print "basename"
    else print "fullpath:" ARGV[0]
}
