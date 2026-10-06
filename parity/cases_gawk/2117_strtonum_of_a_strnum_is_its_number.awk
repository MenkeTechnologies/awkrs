# gawk's do_strtonum returns an operand that is already a number unchanged:
# a numeric string from input (a strnum) is decimal, so the field `017` is 17,
# while the same text as a string constant or a concatenation goes through the
# leading-0 octal detection and is 15. Hex fields are not numeric strings.
{
    print strtonum($1), strtonum($1 ""), strtonum($2), strtonum($3)
    print strtonum(1/3), strtonum("017"), strtonum($4) + 0
}
