# portable:2063
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (86 < 32) + (32 < 50) * 2
    printf "%d\n", int(log(50 + 1) * 10)
    printf "%d\n", match("x2063yz", /[0-9]+/)
}
