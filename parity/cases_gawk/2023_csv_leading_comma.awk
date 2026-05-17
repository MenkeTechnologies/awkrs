BEGIN { FPAT = "([^,]+)|(\"[^\"]+\")" }
{
    print "NF=" NF
    for (i = 1; i <= NF; i++) print i, $i
}
