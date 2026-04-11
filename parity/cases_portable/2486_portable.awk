# portable:2486
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((40 + 13) * 74 / 7)
    { x = "a2486b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
