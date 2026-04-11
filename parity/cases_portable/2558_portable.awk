# portable:2558
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((59 + 59) * 41 / 7)
    { x = "a2558b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
