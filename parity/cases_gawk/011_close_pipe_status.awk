BEGIN {
    cmd = "sh -c 'exit 42'"
    print "hi" | cmd
    r = close(cmd)
    print r
}
