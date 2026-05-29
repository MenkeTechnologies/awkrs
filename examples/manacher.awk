# Manacher's algorithm — longest palindromic substring in O(n).
# Input: one string per line.
# Output: "<input>: pal=<palindrome> len=<n> start=<pos>"
#
# Pre-process by inserting '|' between characters and at both ends so all
# palindromes become odd-length around a center index. p[i] is the half-radius
# of the palindrome centered at the transformed position i.

function manacher(s,   T, n, i, j, center, right, mirror, best_len, best_center, raw_start) {
  T = "|"
  for (i = 1; i <= length(s); i++) T = T substr(s, i, 1) "|"
  n = length(T)
  delete p
  center = 0; right = 0
  best_len = 0; best_center = 0

  for (i = 1; i <= n; i++) {
    if (right > i) {
      mirror = 2 * center - i
      p[i] = (right - i < p[mirror]) ? right - i : p[mirror]
    } else {
      p[i] = 0
    }
    while (i - p[i] - 1 >= 1 && i + p[i] + 1 <= n \
        && substr(T, i - p[i] - 1, 1) == substr(T, i + p[i] + 1, 1)) {
      p[i]++
    }
    if (i + p[i] > right) { center = i; right = i + p[i] }
    if (p[i] > best_len) { best_len = p[i]; best_center = i }
  }

  raw_start = int((best_center - best_len) / 2) + 1
  pal_str = substr(s, raw_start, best_len)
  printf "%s: pal=%s len=%d start=%d\n", s, pal_str, best_len, raw_start
}

NF == 0 { next }
{ manacher($0) }
