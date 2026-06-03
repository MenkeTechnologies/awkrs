# gawk/mawk set ARGV[0] to the binary's basename; BSD `/usr/bin/awk` keeps the
# full path. awkrs matches gawk by default and BSD under `--traditional`.
# Print only a shape marker (never the path) so the diff is byte-stable
# regardless of where each awk lives on disk.
BEGIN {
    if (index(ARGV[0], "/") == 0) print "basename"
    else print "fullpath"
}
