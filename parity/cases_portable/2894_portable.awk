# portable:2894
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((83 + 66) * 53 / 7)
    { x = "a2894b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
