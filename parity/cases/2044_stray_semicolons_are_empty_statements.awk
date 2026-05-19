# gawk accepts bare `;` between statements as an empty statement (C-style).
# Useful when a block-ending `}` is followed by a `;` you forgot to delete.
BEGIN {
    ;
    print "one"
    ;;;
    if (1) { print "two" } ;
    for (i = 0; i < 2; i++) { print "loop", i } ;
    print "done"
}
