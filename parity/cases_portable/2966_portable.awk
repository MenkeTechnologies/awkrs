# portable:2966
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((5 + 23) * 20 / 7)
    { x = "a2966b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
