# portable:2774
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((19 + 19) * 25 / 7)
    { x = "a2774b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
