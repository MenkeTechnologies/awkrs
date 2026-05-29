# Longest Increasing Subsequence (strict) with traceback to recover one LIS.
# O(n log n) via patience-sort-style binary search on the tail array.
# Input: a single line with whitespace-separated integers.
# Output: "LEN: <k>\nLIS: a1 a2 ... ak"

{
  delete a; delete tail; delete prev_idx; delete seq
  n = NF
  if (n == 0) { print "LEN: 0"; print "LIS:"; next }
  for (i = 1; i <= n; i++) a[i] = $i + 0

  # tail[k] = index into `a` of the smallest possible tail of an
  #          increasing subseq of length k.
  # prev_idx[i] = predecessor index of i in the LIS that ends at i.
  tail_count = 0
  for (i = 1; i <= n; i++) {
    lo = 1; hi = tail_count
    while (lo <= hi) {
      mid = int((lo + hi) / 2)
      if (a[tail[mid]] < a[i]) lo = mid + 1
      else hi = mid - 1
    }
    pos = lo
    tail[pos] = i
    prev_idx[i] = (pos == 1) ? 0 : tail[pos - 1]
    if (pos > tail_count) tail_count = pos
  }

  cur = tail[tail_count]
  k = 0
  while (cur != 0) { k++; seq[k] = a[cur]; cur = prev_idx[cur] }

  printf "LEN: %d\n", tail_count
  printf "LIS:"
  for (i = k; i >= 1; i--) printf " %d", seq[i]
  print ""
}
