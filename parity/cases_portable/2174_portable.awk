# portable:2174
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((87 + 51) * 51 / 7)
    { x = "a2174b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
