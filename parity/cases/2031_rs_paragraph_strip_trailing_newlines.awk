# Paragraph mode (RS=="") strips trailing newlines from each record so $0
# does not end in `\n`.
BEGIN { RS = "" }
{
    print NR
    print "len=" length($0)
    print "[" $0 "]"
}
