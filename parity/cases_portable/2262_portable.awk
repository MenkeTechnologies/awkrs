# portable:2262
BEGIN {
    printf "%s\n", tolower("X2262Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (24 < 38) + (38 < 66) * 2
    printf "%d\n", int(log(66 + 1) * 10)
}
