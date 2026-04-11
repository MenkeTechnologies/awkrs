# portable:2870
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((12 + 21) * 64 / 7)
    { x = "a2870b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
