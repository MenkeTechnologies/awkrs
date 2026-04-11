# portable:2702
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((97 + 62) * 58 / 7)
    { x = "a2702b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
