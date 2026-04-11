# portable:2502
BEGIN {
    printf "%s\n", tolower("X2502Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (55 < 43) + (43 < 39) * 2
    printf "%d\n", int(log(39 + 1) * 10)
}
