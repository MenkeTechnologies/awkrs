BEGIN {
    print isarray(ARGV)
    print isarray(ENVIRON)
    x = 1
    print isarray(x)
    delete a
    a[1] = "one"
    print isarray(a)
}
