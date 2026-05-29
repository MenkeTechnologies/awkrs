# Unified-style diff via longest common subsequence + traceback.
# Input layout:
#   === A ===
#   <lines of file A>
#   === B ===
#   <lines of file B>
#
# Output lines:
#   "  ctx"   line common to both
#   "- old"   line only in A
#   "+ new"   line only in B
# Then "DIFF: <a_only> removed, <b_only> added".
#
# DP table c[i,j] held in SUBSEP 2D array; traceback recurses without a stack.

/^=== A ===/ { mode = "A"; next }
/^=== B ===/ { mode = "B"; next }
mode == "A"  { na++; a[na] = $0; next }
mode == "B"  { nb++; b[nb] = $0; next }

function lcs(   i, j) {
  for (i = 0; i <= na; i++) c[i, 0] = 0
  for (j = 0; j <= nb; j++) c[0, j] = 0
  for (i = 1; i <= na; i++) {
    for (j = 1; j <= nb; j++) {
      if (a[i] == b[j]) c[i, j] = c[i - 1, j - 1] + 1
      else c[i, j] = (c[i - 1, j] >= c[i, j - 1]) ? c[i - 1, j] : c[i, j - 1]
    }
  }
}

function emit(i, j) {
  if (i > 0 && j > 0 && a[i] == b[j]) {
    emit(i - 1, j - 1)
    printf "  %s\n", a[i]
    return
  }
  if (j > 0 && (i == 0 || c[i, j - 1] >= c[i - 1, j])) {
    emit(i, j - 1)
    printf "+ %s\n", b[j]
    added++
    return
  }
  if (i > 0 && (j == 0 || c[i, j - 1] <  c[i - 1, j])) {
    emit(i - 1, j)
    printf "- %s\n", a[i]
    removed++
    return
  }
}

END {
  lcs()
  emit(na, nb)
  printf "DIFF: %d removed, %d added\n", removed + 0, added + 0
}
