BEGIN {
    # switch statement
    x = 2
    switch (x) {
    case 1:
        print "one"
        break
    case 2:
        print "two"
        break
    case 3:
        print "three"
        break
    default:
        print "other"
        break
    }

    # switch with string
    s = "hello"
    switch (s) {
    case "hello":
        print "greeting"
        break
    case "bye":
        print "farewell"
        break
    default:
        print "unknown"
        break
    }
}
