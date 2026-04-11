# portable:2366
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((73 + 55) * 46 / 7)
    { x = "a2366b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
