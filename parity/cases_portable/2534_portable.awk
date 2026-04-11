# portable:2534
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((85 + 14) * 52 / 7)
    { x = "a2534b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
