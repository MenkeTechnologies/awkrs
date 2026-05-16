BEGIN {
    if ("0") print "string literal 0 is truthy"
    if (!0) print "number zero is falsy"
    if (!"")  print "empty string is falsy"
}
