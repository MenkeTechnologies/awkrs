# Extended Euclidean algorithm + modular inverse.
# Iterative form so we don't need recursion to return multiple values.
# Input lines:
#   "GCD <a> <b>"     print "gcd(a, b) = g  with  a*x + b*y = g  (x=.., y=..)"
#   "INV <a> <m>"     print "<a>^-1 mod <m> = <r>" or "<a>^-1 mod <m> = NONE"
# Works for positive a, b, m.

function ext_gcd(a, b,   old_r, r, old_s, s, old_t, t, q, tmp) {
  old_r = a; r = b
  old_s = 1; s = 0
  old_t = 0; t = 1
  while (r != 0) {
    q = int(old_r / r)
    tmp = r; r = old_r - q * r; old_r = tmp
    tmp = s; s = old_s - q * s; old_s = tmp
    tmp = t; t = old_t - q * t; old_t = tmp
  }
  gx = old_s; gy = old_t
  return old_r
}

function mod_inv(a, m,   g, inv) {
  g = ext_gcd(a, m)
  if (g != 1) return ""
  inv = gx
  inv = (inv % m + m) % m
  return inv
}

$1 == "GCD" {
  a = $2 + 0; b = $3 + 0
  g = ext_gcd(a, b)
  printf "gcd(%d, %d) = %d  with  %d*%d + %d*%d = %d\n", a, b, g, a, gx, b, gy, g
  next
}
$1 == "INV" {
  a = $2 + 0; m = $3 + 0
  r = mod_inv(a, m)
  if (r == "") printf "%d^-1 mod %d = NONE\n", a, m
  else         printf "%d^-1 mod %d = %d\n", a, m, r
  next
}
