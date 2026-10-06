# gawk do_sub scans a sub/gsub replacement left to right: 3 backslashes + & give
# a literal backslash-&, 4 backslashes give 2, 2 + & give a backslash then the
# match, 1 + & gives &, and any other backslash is copied as is.
BEGIN {
  r[1] = "\\\\"; r[2] = "\\\\\\\\"; r[3] = "a\\\\b"; r[4] = "\\\\\\"; r[5] = "\\\\x\\\\"; r[6] = "\\\\\\\\&"; r[7]="\\\\\\\\\\\\"; r[8] = "\\&"; r[9] = "\\\\&"; r[10] = "\\q"; r[11] = "x\\"
  for (i = 1; i <= 11; i++) {
    s = "xAy"; sub(/A/, r[i], s); t = "xAyA"; gsub(/A/, r[i], t)
    printf "%d %s sub=[%s] gsub=[%s]\n", i, r[i], s, t
  }
}
