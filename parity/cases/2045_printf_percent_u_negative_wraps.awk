# gawk: %u of a negative value wraps via i64→u64 (two's complement), not 0.
BEGIN {
    printf "%u\n", -1
    printf "%u\n", -5
    printf "%u\n", 5
    printf "%u\n", 0
    printf "%u\n", 2147483648
}
