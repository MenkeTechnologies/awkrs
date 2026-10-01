# gawk's `awk::` prefix names the global variable, array or function: `awk::z`
# and `z` are one variable.
function top(x) { return x * 2 }
BEGIN {
  z = 4; awk::w = 6
  print awk::z, w, awk::top(21)
  awk::list["k"] = 1; list["j"] = 2; print length(list), length(awk::list)
  awk::n++; n++; print n
}
