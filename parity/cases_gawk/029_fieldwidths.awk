BEGIN {
    # FIELDWIDTHS-based splitting
    FIELDWIDTHS = "3 4 5"
}
{
    # Only check the first two fields to avoid last-field-length divergence
    print "f1=[" $1 "] f2=[" $2 "]"
}
