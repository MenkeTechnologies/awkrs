# Z-function in O(n): z[i] = length of the longest substring starting at i
# that matches a prefix of s. Used for linear-time pattern matching.
#
# Input lines:  "<string>"
# Output:       "<string>: z=[z1,z2,...]"
#               If the string contains '#' followed by "PAT:" then split into
#               pattern + text and report match positions:
#                 "MATCHES: pos1 pos2 ..."

function z_array(s,   n, i, l, r, k) {
  n = length(s)
  delete z
  z[1] = n
  l = 0; r = 0
  for (i = 2; i <= n; i++) {
    if (i <= r) z[i] = (r - i + 1 < z[i - l + 1]) ? r - i + 1 : z[i - l + 1]
    else        z[i] = 0
    while (i + z[i] <= n && substr(s, z[i] + 1, 1) == substr(s, i + z[i], 1)) z[i]++
    if (i + z[i] - 1 > r) { l = i; r = i + z[i] - 1 }
  }
  return n
}

function show_z(s,   n, i, out, sep) {
  n = z_array(s)
  out = ""; sep = ""
  for (i = 1; i <= n; i++) { out = out sep z[i]; sep = "," }
  printf "%s: z=[%s]\n", s, out
}

function find_matches(pat, txt,   sep_pos, n, m, s, i, hits, sep) {
  m = length(pat)
  s = pat "#" txt
  n = z_array(s)
  hits = ""; sep = ""
  for (i = m + 2; i <= n; i++) {
    if (z[i] >= m) {
      hits = hits sep (i - m - 1)
      sep = " "
    }
  }
  return hits == "" ? "NONE" : hits
}

NF == 0 { next }
{
  line = $0
  if (index(line, " in ")) {
    pat = $1
    sub(/^[^ ]+ in /, "", line)
    txt = line
    show_z(pat)
    printf "MATCHES of %s in %s: %s\n", pat, txt, find_matches(pat, txt)
    next
  }
  show_z(line)
}
