BEGIN {
    print or(1, 2)
    print or(1, 2, 4)
    print or(0, 0, 0, 1)
    print or(128, 64, 32, 16)
}
