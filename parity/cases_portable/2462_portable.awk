# portable:2462
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((66 + 57) * 85 / 7)
    { x = "a2462b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
