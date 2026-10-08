# portable:3013 — backslash-newline inside a string literal continues the
# string on the next line and adds nothing to it.
BEGIN {
    print "con\
tinued"
    x = "a\
\
b"
    print x, length(x)
    print ("ab" ~ "a\
b")
}
