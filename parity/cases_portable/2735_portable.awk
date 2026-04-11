# portable:2735
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (37 < 46) + (46 < 74) * 2
    printf "%d\n", int(log(74 + 1) * 10)
    printf "%d\n", match("x2735yz", /[0-9]+/)
}
