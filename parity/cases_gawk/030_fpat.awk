BEGIN {
    # FPAT-based splitting — simple non-quoted fields
    FPAT = "[^,]+"
}
{
    for (i = 1; i <= NF; i++) printf "f%d=[%s] ", i, $i
    print ""
}
