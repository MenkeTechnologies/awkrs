# portable:2654
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((52 + 61) * 80 / 7)
    { x = "a2654b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
