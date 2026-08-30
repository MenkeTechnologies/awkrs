
# portable:3003 — plain `getline` vs `getline var`.
# The two forms differ in what they touch: plain sets $0 and NF as well as
# NR/FNR, while the var form sets only the variable and NR/FNR, leaving the
# current record and its fields exactly as they were.
NR == 1 {
    printf "before: NR=%d FNR=%d NF=%d $0=[%s]\n", NR, FNR, NF, $0
    r = getline
    printf "plain : ret=%d NR=%d FNR=%d NF=%d $0=[%s] $1=[%s]\n", r, NR, FNR, NF, $0, $1
    r = getline line
    printf "var   : ret=%d NR=%d FNR=%d NF=%d $0=[%s] line=[%s]\n", r, NR, FNR, NF, $0, line
    # At end of input the return is 0, and nothing is disturbed.
    while ((getline) > 0) { }
    r = getline eof_line
    printf "eof   : ret=%d NR=%d $0=[%s] eof_line=[%s]\n", r, NR, $0, eof_line
}
END { printf "end   : NR=%d\n", NR }
