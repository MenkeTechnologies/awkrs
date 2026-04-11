# portable:2942
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
    printf "%d\n", int((31 + 67) * 31 / 7)
    { x = "a2942b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
