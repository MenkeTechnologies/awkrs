# portable:2471
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (32 < 85) + (85 < 29) * 2
    printf "%d\n", int(log(29 + 1) * 10)
    printf "%d\n", match("x2471yz", /[0-9]+/)
}
