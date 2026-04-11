# portable:2773
BEGIN {
    printf "%s\n", sprintf("%02x", 2 + 10)
    printf "%s\n", tolower("X2773Y")
    printf "%d\n", ("ab" < "ac") + ("x" == "x")
    printf "%d\n", (12 < 6) + (6 < 22) * 2
}
