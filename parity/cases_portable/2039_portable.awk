# portable:2039
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (15 < 76) + (76 < 61) * 2
    printf "%d\n", int(log(61 + 1) * 10)
    printf "%d\n", match("x2039yz", /[0-9]+/)
}
