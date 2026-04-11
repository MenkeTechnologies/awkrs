# portable:2078
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((94 + 49) * 12 / 7)
    { x = "a2078b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
