# portable:2438
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((92 + 12) * 13 / 7)
    { x = "a2438b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
