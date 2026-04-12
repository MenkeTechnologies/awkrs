BEGIN {
    # length of string
    print length("hello")
    print length("")
    print length("abc def")

    # length of array
    a[1] = "x"
    a[2] = "y"
    a[3] = "z"
    print length(a)

    # delete and check
    delete a[2]
    print length(a)
}
