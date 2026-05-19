# Buffered awk output must be flushed before `system()` runs so the subprocess
# output is correctly interleaved.
BEGIN {
    print "before"
    system("echo middle")
    print "after"
}
