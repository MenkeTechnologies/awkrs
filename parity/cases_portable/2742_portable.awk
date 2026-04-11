# portable:2742
BEGIN {
    printf "%s\n", tolower("X2742Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (86 < 48) + (48 < 12) * 2
    printf "%d\n", int(log(12 + 1) * 10)
}
