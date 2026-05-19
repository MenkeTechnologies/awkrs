# POSIX: `printf "%.0d", 0` produces zero digits (an empty field). Applies to
# %d, %i, %u, %o, %x as well. Non-zero values still emit their digits.
BEGIN {
    printf "[%.0d|%.0i|%.0u|%.0o|%.0x|%.0X]\n", 0, 0, 0, 0, 0, 0
    printf "[%.0d|%.0u|%.0x]\n", 5, 5, 255

    # Combined with width — POSIX padding still applies to the empty field.
    printf "[%5.0d|%-5.0d]\n", 0, 0
    printf "[%5.0d|%-5.0d]\n", 7, 7
}
