BEGIN { FPAT = "[a-z]+" }
{
    print NF
    for (i = 1; i <= NF; i++) print "[" i "]=" $i
}
