BEGIN {
    printf "%a\n", 1.5
    printf "%a\n", 3.14
    printf "%a\n", 0.0
    printf "%a\n", -1.0
    printf "%a\n", 1.0
    printf "%a\n", 2.0
    printf "%a\n", 0.5
    printf "%a\n", 0.125
    printf "%a\n", 1024.0
    printf "%a\n", -0.0
    printf "%.4a\n", 1.5
    # %.0a rounding is platform-dependent (glibc rounds, Apple libc truncates);
    # awkrs rounds correctly — tested in Rust unit tests instead.
    printf "%A\n", 255.0
    printf "%A\n", 1.0
    printf "%20a\n", 1.5
    printf "%-20a\n", 1.5
}
