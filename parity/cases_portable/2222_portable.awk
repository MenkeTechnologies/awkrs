# portable:2222
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((35 + 52) * 29 / 7)
    { x = "a2222b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
