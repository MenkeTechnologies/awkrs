# gawk arrays of arrays: per-key subarrays built from input, nested for-in in
# sorted_in order, `in` / `delete` / `length` / `isarray` at depth, a recursive
# walk that passes subarrays by reference, SUBSEP keys inside a path, and the
# builtins that fill or edit a subarray element (split, asort, sub).
function walk(arr, pre,   k) {
  for (k in arr)
    if (isarray(arr[k])) walk(arr[k], pre k "/")
    else print pre k " = " arr[k]
}
function total(arr,   k, s) { for (k in arr) s += isarray(arr[k]) ? total(arr[k]) : arr[k]; return s }
{ sales[$1][$2] += $3; seen[$1][$2][NR] = $3 }
END {
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (r in sales) { printf "%s:", r; for (p in sales[r]) printf " %s=%d", p, sales[r][p]; print "" }
  walk(seen, "")
  print total(sales), length(sales), length(sales["east"]), isarray(sales["east"]), isarray(sales["east"]["pen"])
  print ("pen" in sales["east"]), ("cup" in sales["east"]), ((1, 2) in grid)
  grid[1, 2]["z"] = 9; print ((1, 2) in grid), grid[1, 2]["z"]
  delete sales["east"]["pen"]; print length(sales["east"])
  delete sales["west"]; print length(sales), ("west" in sales)
  sales["north"]["ink"]++; sales["north"]["ink"] *= 5; print sales["north"]["ink"]--, sales["north"]["ink"]
  n = split("c a b", words["list"]); print n, asort(words["list"]), words["list"][1] words["list"][3]
  s["x"]["y"] = "hello"; print gsub(/l/, "L", s["x"]["y"]), s["x"]["y"]
}
