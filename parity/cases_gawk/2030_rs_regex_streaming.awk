BEGIN {
    RS = "[|,;]"
}
{
    print NR, "rec=[" $0 "]", "RT=[" RT "]"
}
