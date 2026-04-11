# portable:2375
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (39 < 83) + (83 < 73) * 2
    printf "%d\n", int(log(73 + 1) * 10)
    printf "%d\n", match("x2375yz", /[0-9]+/)
}
