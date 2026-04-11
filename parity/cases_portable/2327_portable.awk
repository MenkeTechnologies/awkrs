# portable:2327
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (91 < 82) + (82 < 12) * 2
    printf "%d\n", int(log(12 + 1) * 10)
    printf "%d\n", match("x2327yz", /[0-9]+/)
}
