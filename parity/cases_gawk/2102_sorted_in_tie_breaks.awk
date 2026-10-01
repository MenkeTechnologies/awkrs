# PROCINFO["sorted_in"]: equal values are ordered by the index string, a
# descending mode reverses the whole order including ties, equal numeric
# indices fall back to the index string, and unassigned elements sort by type
# below every scalar.
function walk(mode,   k) { PROCINFO["sorted_in"] = mode; printf "%-14s", mode; for (k in a) printf " %s", k; print "" }
BEGIN {
  a["x"] = 1; a["y"] = 1; a["b"] = 1; a["c"] = 2; a["d"] = "s"; a["e"]; a["f"] = "1"
  walk("@val_num_asc"); walk("@val_num_desc"); walk("@val_str_asc"); walk("@val_str_desc")
  walk("@val_type_asc"); walk("@val_type_desc")
  delete a
  a["10"]; a["1e1"]; a["010"]; a[9]; a["abc"]
  walk("@ind_num_asc"); walk("@ind_num_desc"); walk("@ind_str_desc")
}
