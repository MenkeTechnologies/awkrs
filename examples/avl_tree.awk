# AVL self-balancing binary search tree.
# Input lines:
#   "INSERT <k>"     insert key k (deduped)
#   "INORDER"        print sorted keys: "INORDER: k1 k2 ..."
#   "HEIGHT"         print height of the root: "HEIGHT: h"
#   "BALANCED"       verify every node satisfies |bf| <= 1: prints YES/NO
#
# Node arrays:
#   key[i] left[i] right[i] h[i]   (0 = nil)

function ht(n) { return (n == 0) ? 0 : h[n] }
function bf(n) { return (n == 0) ? 0 : ht(left[n]) - ht(right[n]) }
function update_h(n) {
  if (n == 0) return
  h[n] = 1 + ((ht(left[n]) > ht(right[n])) ? ht(left[n]) : ht(right[n]))
}

function rot_right(y,   x) {
  x = left[y]
  left[y] = right[x]
  right[x] = y
  update_h(y)
  update_h(x)
  return x
}

function rot_left(x,   y) {
  y = right[x]
  right[x] = left[y]
  left[y] = x
  update_h(x)
  update_h(y)
  return y
}

function rebalance(n,   b) {
  update_h(n)
  b = bf(n)
  if (b > 1 && bf(left[n]) >= 0) return rot_right(n)
  if (b > 1 && bf(left[n]) <  0) { left[n] = rot_left(left[n]); return rot_right(n) }
  if (b < -1 && bf(right[n]) <= 0) return rot_left(n)
  if (b < -1 && bf(right[n]) >  0) { right[n] = rot_right(right[n]); return rot_left(n) }
  return n
}

function insert(n, k) {
  if (n == 0) {
    cnt++
    key[cnt] = k; left[cnt] = 0; right[cnt] = 0; h[cnt] = 1
    return cnt
  }
  if (k < key[n])      left[n]  = insert(left[n], k)
  else if (k > key[n]) right[n] = insert(right[n], k)
  else return n
  return rebalance(n)
}

function inorder(n) {
  if (n == 0) return
  inorder(left[n])
  inorder_out = inorder_out " " key[n]
  inorder(right[n])
}

function check_balanced(n,   b) {
  if (n == 0) return 1
  b = bf(n)
  if (b < -1 || b > 1) return 0
  if (!check_balanced(left[n])) return 0
  return check_balanced(right[n])
}

BEGIN { root = 0; cnt = 0 }

$1 == "INSERT" { root = insert(root, $2 + 0); next }
$1 == "INORDER" {
  inorder_out = ""
  inorder(root)
  sub(/^ /, "", inorder_out)
  print "INORDER: " inorder_out
  next
}
$1 == "HEIGHT" { print "HEIGHT: " ht(root); next }
$1 == "BALANCED" { print "BALANCED: " (check_balanced(root) ? "YES" : "NO"); next }
