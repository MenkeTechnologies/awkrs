# portable:2975
BEGIN {
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (68 < 51) + (51 < 47) * 2
    printf "%d\n", int(log(47 + 1) * 10)
    printf "%d\n", match("x2975yz", /[0-9]+/)
}
