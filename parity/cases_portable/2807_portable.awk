# portable:2807
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (56 < 3) + (3 < 41) * 2
    printf "%d\n", int(log(41 + 1) * 10)
    printf "%d\n", match("x2807yz", /[0-9]+/)
}
