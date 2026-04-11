# portable:2282
BEGIN {
    printf "%d\n", match("x2282yz", /[0-9]+/)
    { a1[1] = 67; a1[2] = 31; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(67 + 1.0)))
    printf "%d\n", split("67:31:43", t, ":") + length(t[2])
}
