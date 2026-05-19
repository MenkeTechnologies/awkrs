# gawk parity: the `'` flag groups the integer portion of `%f` / `%e` / `%g`
# values, leaving anything after the radix untouched. Previously awkrs only
# honored `'` on integer conversions.
BEGIN {
    printf "%'f\n", 1234567.89
    printf "%'.2f\n", 9876543.21
    printf "%'d\n", 1234567
    printf "%'.6f\n", 1.5
}
