# portable:2678
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((26 + 17) * 69 / 7)
    { x = "a2678b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
