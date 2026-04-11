# portable:2918
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((57 + 22) * 42 / 7)
    { x = "a2918b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
