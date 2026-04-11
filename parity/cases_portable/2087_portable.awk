# portable:2087
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (60 < 77) + (77 < 39) * 2
    printf "%d\n", int(log(39 + 1) * 10)
    printf "%d\n", match("x2087yz", /[0-9]+/)
}
