# gawk parity: close() on a name that's not currently open returns -1.
BEGIN {
    # No handle ever opened.
    print close("/tmp/awkrs_definitely_not_open_x9z_path")

    # Open then close — first close succeeds (0), second returns -1.
    out = "/tmp/awkrs_close_parity_demo.txt"
    print "first line" > out
    print close(out)
    print close(out)
    # Cleanup is the caller's responsibility; not removing here keeps the script side-effect.
}
