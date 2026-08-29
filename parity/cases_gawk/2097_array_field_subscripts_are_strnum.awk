# A field used as a subscript is a numeric string: it compares numerically, and
# it names the element its NUMBER renders to — so `$1` of "01" is not the same
# element as `$1` of "1", because the rendering differs, not the value.
{ a[$1] = NR }
END {
    PROCINFO["sorted_in"] = "@ind_str_asc"
    print length(a)
    for (k in a) printf "  [%s]=%s\n", k, a[k]
    print (1 in a), ("1" in a), ("01" in a), (2 in a)
}
