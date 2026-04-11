# portable:2390
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((47 + 11) * 35 / 7)
    { x = "a2390b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
