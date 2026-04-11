# portable:2030
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab0c")
    printf "%d\n", int((49 + 48) * 34 / 7)
    { x = "a2030b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
