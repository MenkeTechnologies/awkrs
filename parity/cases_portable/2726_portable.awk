# portable:2726
BEGIN {
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab1c")
    printf "%d\n", int((71 + 18) * 47 / 7)
    { x = "a2726b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
