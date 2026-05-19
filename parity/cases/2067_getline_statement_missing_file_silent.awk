# gawk parity: `getline var < missing_file` as a STATEMENT silently sets
# ERRNO and continues. Only the expression form `(getline var < file)` returns
# -1; the statement form must not abort the program.
BEGIN {
    line = "before"
    getline line < "/tmp/awkrs_definitely_does_not_exist_xyz_zzz"
    print "line:", line
    print "ERRNO:", ERRNO
}
