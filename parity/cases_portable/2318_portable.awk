# portable:2318
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((28 + 54) * 68 / 7)
    { x = "a2318b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
