# portable:2270
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((80 + 53) * 7 / 7)
    { x = "a2270b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
