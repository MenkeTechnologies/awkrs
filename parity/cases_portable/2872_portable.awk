# portable:2872
BEGIN {
    printf "%d\n", (26 < 47) + (47 < 70) * 2
    printf "%d\n", int(log(70 + 1) * 10)
    printf "%d\n", match("x2872yz", /[0-9]+/)
    { a1[1] = 26; a1[2] = 47; printf "%d\n", a1[1] + a1[2] }
}
