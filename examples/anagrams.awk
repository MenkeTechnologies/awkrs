# Group anagrams from a word list.
# Input: one word per line.
# Output: each anagram group on its own line, words in input order,
# groups sorted by their canonical (letter-sorted) signature.
# Then "GROUPS: <k>".
# Signature: characters of the word sorted ascending.

function sig(w,   n, i, a, sorted, key, j) {
  n = length(w)
  for (i = 1; i <= n; i++) a[i] = substr(w, i, 1)
  for (i = 2; i <= n; i++) {
    key = a[i]; j = i - 1
    while (j >= 1 && a[j] > key) { a[j + 1] = a[j]; j-- }
    a[j + 1] = key
  }
  sorted = ""
  for (i = 1; i <= n; i++) sorted = sorted a[i]
  return sorted
}

NF == 0 { next }
{
  w = tolower($1)
  s = sig(w)
  group[s] = group[s] ((s in seen) ? " " : "") w
  seen[s] = 1
}

END {
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (s in group) { print group[s]; n++ }
  printf "GROUPS: %d\n", n
}
