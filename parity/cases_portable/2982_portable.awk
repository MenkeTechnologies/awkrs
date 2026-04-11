# portable:2982
BEGIN {
    printf "%s\n", tolower("X2982Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (20 < 53) + (53 < 68) * 2
    printf "%d\n", int(log(68 + 1) * 10)
}
