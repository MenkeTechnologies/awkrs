# portable:2198
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((61 + 7) * 40 / 7)
    { x = "a2198b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
