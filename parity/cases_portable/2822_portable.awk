# portable:2822
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((64 + 20) * 3 / 7)
    { x = "a2822b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
