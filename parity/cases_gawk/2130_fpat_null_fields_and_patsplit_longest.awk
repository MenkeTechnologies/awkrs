# gawk:2130 — FPAT and patsplit() share gawk's fpat_parse_field: a pattern
# that can match the empty string yields empty fields between separators
# (`a,,b,` under `[^,]*` is a, "", b, ""), and patsplit() matches
# leftmost-longest like FPAT, so the manual's quoted-CSV pattern keeps
# `"def, ghi"` whole instead of splitting it at the comma.
BEGIN { FPAT = "[^,]*" }
{
    printf "%d:", NF
    for (i = 1; i <= NF; i++) printf "[%s]", $i
    n = patsplit($0, p, /([^,]*)|("[^"]+")/, s)
    printf " patsplit %d:", n
    for (i = 0; i <= n; i++) printf "[%s|%s]", p[i], s[i]
    print ""
}
