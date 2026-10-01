# PROCINFO["sorted_in"] orders `for (k in arr)` over a function's array
# parameter and over a local array exactly as over a global.
function show(arr,   k) { for (k in arr) printf "%s ", k; print "" }
function local_copy(src,   loc, k) { for (k in src) loc[k] = src[k]; show(loc); for (k in loc) printf "%s=%s ", k, loc[k]; print "" }
function by_len(i1, v1, i2, v2) { if (length(i1) != length(i2)) return length(i1) - length(i2); return i1 < i2 ? -1 : i1 > i2 }
BEGIN {
  for (i = 1; i <= 12; i++) a["k" i] = (i * 7) % 13
  PROCINFO["sorted_in"] = "@ind_str_asc"; show(a)
  PROCINFO["sorted_in"] = "@val_num_desc"; show(a)
  PROCINFO["sorted_in"] = "@ind_str_desc"; local_copy(a)
  PROCINFO["sorted_in"] = "by_len"; show(a)
}
