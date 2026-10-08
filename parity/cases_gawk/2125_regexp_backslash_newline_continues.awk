# gawk:2125 — backslash-newline inside a regexp literal continues it on the
# next line (gawk and mawk; one-true-awk rejects it).
BEGIN {
    print ("ab" ~ /a\
b/), ("a\nb" ~ /^a\
b$/)
    print "c\
d", ("cd" ~ /c\
d/)
}
