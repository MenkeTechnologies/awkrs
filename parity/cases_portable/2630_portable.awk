# portable:2630
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((78 + 16) * 8 / 7)
    { x = "a2630b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
