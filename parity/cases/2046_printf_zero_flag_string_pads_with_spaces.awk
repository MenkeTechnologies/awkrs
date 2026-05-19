# POSIX: `0` flag is for numeric conversions only. On %s and %c it is ignored
# and the field still pads with spaces.
BEGIN {
    printf "[%05s][%-05s][%05c]\n", "ab", "cd", 65
    printf "[%010d]|[%010s]\n", 42, "42"
}
