# portable:3000 — a numeric subscript and the string of its rendering name one
# element; a spelling that is not that rendering names its own. Checked with
# explicit `in` tests rather than iteration, whose order awk does not define.
BEGIN {
    a[1] = "int"
    a["1"] = "str"
    printf "%s %s\n", a[1], a["1"]
    a["01"] = "oh1"
    a["+1"] = "plus1"
    a["1.0"] = "onedot"
    printf "%s %s %s %s\n", a[1], a["01"], a["+1"], a["1.0"]
    printf "%d %d %d %d\n", (1 in a), ("1" in a), ("01" in a), ("2" in a)
    b[1.0] = "float-one"
    printf "%d %d %s\n", (1 in b), ("1" in b), b[1]
    c[-0] = "negzero"
    printf "%d %s\n", (0 in c), c[0]
    d[-7] = "neg"
    printf "%d %d %s\n", (-7 in d), ("-7" in d), d["-7"]
}
