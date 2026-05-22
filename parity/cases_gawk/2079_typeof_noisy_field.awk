# gawk parity: a field whose text has a numeric prefix BUT extra non-numeric
# characters is NOT a "numeric string" — typeof reports "string" (not "strnum"),
# and equality / relational compares run as string compares.
{
    print typeof($1)
    print ($1 == 42)
    print ($1 == "42abc")
    print ($1 < 100) ? "lt" : "ge"
}
