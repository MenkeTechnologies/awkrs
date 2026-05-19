# `%g` precision is the total significant digit count (C99 / POSIX).
# Precision 0 is treated as 1.
BEGIN {
    printf "%.0g|%.1g|%.2g|%.3g|%.6g\n", 123.456, 123.456, 123.456, 123.456, 123.456
    printf "%.0g|%.1g|%.2g\n", 0.000123, 0.000123, 0.000123
    printf "%g|%G\n", 1e10, 1e10
    printf "%.1g|%.1G\n", 9.5, 9.5
}
