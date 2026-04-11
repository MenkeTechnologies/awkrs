# portable:2927
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (23 < 50) + (50 < 69) * 2
    printf "%d\n", int(log(69 + 1) * 10)
    printf "%d\n", match("x2927yz", /[0-9]+/)
}
