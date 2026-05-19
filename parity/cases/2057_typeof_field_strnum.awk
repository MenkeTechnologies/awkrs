# gawk parity for typeof() on fields:
#   - "strnum" if the field's string parses as a number
#   - "string" if not
#   - "unassigned" for fields past NF
{
    printf "$1=%s $2=%s $3=%s $4=%s $5=%s\n", typeof($1), typeof($2), typeof($3), typeof($4), typeof($5)
}
