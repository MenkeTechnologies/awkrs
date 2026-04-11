# portable:2478
BEGIN {
    printf "%s\n", tolower("X2478Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (81 < 87) + (87 < 50) * 2
    printf "%d\n", int(log(50 + 1) * 10)
}
