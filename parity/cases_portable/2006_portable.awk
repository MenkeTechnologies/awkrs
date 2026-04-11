# portable:2006
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((75 + 3) * 45 / 7)
    { x = "a2006b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
