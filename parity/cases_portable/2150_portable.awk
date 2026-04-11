# portable:2150
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((16 + 6) * 62 / 7)
    { x = "a2150b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
