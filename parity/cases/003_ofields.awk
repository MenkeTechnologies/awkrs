BEGIN {
    OFS = ":"
    $0 = "a b"
    print $1, $2
}
