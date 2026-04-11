# portable:2750
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((45 + 63) * 36 / 7)
    { x = "a2750b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
