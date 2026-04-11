# portable:2598
BEGIN {
    printf "%s\n", tolower("X2598Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (48 < 45) + (45 < 78) * 2
    printf "%d\n", int(log(78 + 1) * 10)
}
