# Convex hull via Andrew's monotone chain (lower + upper hull, O(n log n)).
# Input lines:  "<x> <y>" (one point per line; integer coords are fine).
# Output:       hull vertices in CCW order starting at the lex-min point,
#               then "AREA: <area>" computed by the shoelace formula.

NF == 2 {
  n++
  px[n] = $1 + 0
  py[n] = $2 + 0
}

# cross((O,A), (O,B)) > 0 iff A is to the left of OB.
function cross(ox, oy, ax, ay, bx, by) {
  return (ax - ox) * (by - oy) - (ay - oy) * (bx - ox)
}

END {
  # Sort points by (x, y) using insertion sort on a parallel index array —
  # asort would re-index by value which loses the (x, y) pairing.
  for (i = 1; i <= n; i++) idx[i] = i
  for (i = 2; i <= n; i++) {
    k = idx[i]; j = i - 1
    while (j >= 1 \
      && (px[idx[j]] > px[k] \
          || (px[idx[j]] == px[k] && py[idx[j]] > py[k]))) {
      idx[j + 1] = idx[j]; j--
    }
    idx[j + 1] = k
  }

  # Lower hull.
  k = 0
  for (i = 1; i <= n; i++) {
    p = idx[i]
    while (k >= 2 \
      && cross(px[hull[k-1]], py[hull[k-1]], px[hull[k]], py[hull[k]], px[p], py[p]) <= 0) {
      delete hull[k]; k--
    }
    k++; hull[k] = p
  }

  # Upper hull.
  lower_count = k
  for (i = n - 1; i >= 1; i--) {
    p = idx[i]
    while (k > lower_count \
      && cross(px[hull[k-1]], py[hull[k-1]], px[hull[k]], py[hull[k]], px[p], py[p]) <= 0) {
      delete hull[k]; k--
    }
    k++; hull[k] = p
  }
  k--   # last point is the start point — drop it

  # Emit hull verts CCW from lex-min point.
  for (i = 1; i <= k; i++) printf "(%d, %d)\n", px[hull[i]], py[hull[i]]

  # Shoelace area; 2A = |sum_i (xi * y_{i+1} - x_{i+1} * yi)|
  area2 = 0
  for (i = 1; i <= k; i++) {
    j = (i % k) + 1
    area2 += px[hull[i]] * py[hull[j]] - px[hull[j]] * py[hull[i]]
  }
  if (area2 < 0) area2 = -area2
  printf "AREA: %.1f\n", area2 / 2
}
