# portable:2990
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((76 + 68) * 9 / 7)
    { x = "a2990b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
