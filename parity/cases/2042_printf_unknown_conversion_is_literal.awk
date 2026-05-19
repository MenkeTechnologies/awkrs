# gawk: unknown conversion letters like %q emit the literal `%X` and do NOT
# consume an argument — so the next valid conversion still sees the user's args.
# (gawk has some additional fatal-error behavior for unknown conversions in
# certain positions when args run out — that corner is intentionally not
# exercised here since it's quirky and varies across gawk releases.)
BEGIN {
    printf "[%q][%s][%z]\n", "first", "second"
    printf "%v\n", "ignored"
}
