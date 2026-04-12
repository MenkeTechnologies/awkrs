BEGIN {
    print lshift(1, 0)
    print lshift(1, 1)
    print lshift(1, 8)
    print lshift(1, 16)
    print rshift(256, 1)
    print rshift(256, 8)
    print rshift(1024, 4)
    print rshift(0, 5)
}
