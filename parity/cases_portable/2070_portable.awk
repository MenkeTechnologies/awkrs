# portable:2070
BEGIN {
    printf "%s\n", tolower("X2070Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (38 < 34) + (34 < 71) * 2
    printf "%d\n", int(log(71 + 1) * 10)
}
