# portable:2622
BEGIN {
    printf "%s\n", tolower("X2622Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (22 < 90) + (90 < 67) * 2
    printf "%d\n", int(log(67 + 1) * 10)
}
