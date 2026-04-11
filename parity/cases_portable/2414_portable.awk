# portable:2414
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((21 + 56) * 24 / 7)
    { x = "a2414b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
