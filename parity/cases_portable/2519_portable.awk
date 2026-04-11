# portable:2519
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (77 < 86) + (86 < 7) * 2
    printf "%d\n", int(log(7 + 1) * 10)
    printf "%d\n", match("x2519yz", /[0-9]+/)
}
