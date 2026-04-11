# portable:2399
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (13 < 39) + (39 < 62) * 2
    printf "%d\n", int(log(62 + 1) * 10)
    printf "%d\n", match("x2399yz", /[0-9]+/)
}
