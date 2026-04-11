# portable:2018
BEGIN {
    printf "%d\n", match("x2018yz", /[0-9]+/)
    { a1[1] = 62; a1[2] = 70; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(62 + 1.0)))
    printf "%d\n", split("62:70:81", t, ":") + length(t[2])
}
