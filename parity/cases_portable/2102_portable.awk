# portable:2102
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((68 + 5) * 84 / 7)
    { x = "a2102b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
