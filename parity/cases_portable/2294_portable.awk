# portable:2294
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab4c")
    printf "%d\n", int((54 + 9) * 79 / 7)
    { x = "a2294b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
