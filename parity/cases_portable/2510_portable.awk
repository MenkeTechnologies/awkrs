# portable:2510
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((14 + 58) * 63 / 7)
    { x = "a2510b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
