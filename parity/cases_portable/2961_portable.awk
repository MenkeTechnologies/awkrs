# portable:2961
BEGIN {
    printf "%d\n", int(log(5 + 1) * 10)
    printf "%d\n", match("x2961yz", /[0-9]+/)
    { a1[1] = 67; a1[2] = 47; printf "%d\n", a1[1] + a1[2] }
    printf "%d\n", int(exp(log(67 + 1.0)))
}
