# portable:2054
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((23 + 4) * 23 / 7)
    { x = "a2054b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
