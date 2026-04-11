# portable:2606
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((7 + 60) * 19 / 7)
    { x = "a2606b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
