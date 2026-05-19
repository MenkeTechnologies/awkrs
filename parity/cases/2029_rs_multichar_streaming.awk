BEGIN {
    RS = "\n---\n"
}
{
    print NR, "[" $0 "]"
}
