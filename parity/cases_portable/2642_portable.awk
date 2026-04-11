# portable:2642
BEGIN {
    printf "%d\n", match("x2642yz", /[0-9]+/)
    { a1[1] = 65; a1[2] = 83; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(65 + 1.0)))
    printf "%d\n", split("65:83:44", t, ":") + length(t[2])
}
