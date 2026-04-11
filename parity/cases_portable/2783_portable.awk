# portable:2783
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (82 < 47) + (47 < 52) * 2
    printf "%d\n", int(log(52 + 1) * 10)
    printf "%d\n", match("x2783yz", /[0-9]+/)
}
