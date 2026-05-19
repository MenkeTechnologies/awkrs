# POSIX / gawk: the `#` flag adds the `0x` / `0X` / `0` prefix only when the
# value is non-zero. `printf "%#x", 0` produces "0", not "0x0".
BEGIN {
    printf "%#x|%#X|%#o\n", 0, 0, 0
    printf "%#x|%#X|%#o\n", 255, 255, 8
    printf "%#10x|%#-10x\n", 255, 255
}
