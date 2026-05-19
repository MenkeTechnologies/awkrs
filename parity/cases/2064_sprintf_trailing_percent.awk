# gawk parity: a format string ending in `%` (no conversion letter after it)
# emits the literal `%` rather than raising "truncated format".
BEGIN {
    print sprintf("abc%")
    print sprintf("%d%%done%", 50)
    printf "[%]\n"
}
