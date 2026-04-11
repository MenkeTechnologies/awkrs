# portable:2831
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (30 < 48) + (48 < 30) * 2
    printf "%d\n", int(log(30 + 1) * 10)
    printf "%d\n", match("x2831yz", /[0-9]+/)
}
