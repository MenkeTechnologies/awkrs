# portable:2126
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((42 + 50) * 73 / 7)
    { x = "a2126b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
