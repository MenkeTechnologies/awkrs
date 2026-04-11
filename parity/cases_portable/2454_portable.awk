# portable:2454
BEGIN {
    printf "%s\n", tolower("X2454Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (10 < 42) + (42 < 61) * 2
    printf "%d\n", int(log(61 + 1) * 10)
}
