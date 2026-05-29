# Modular exponentiation + deterministic Miller-Rabin primality test for
# 64-bit integers. The witness set used here (2, 3, 5, 7, 11, 13, 17, 19, 23,
# 29, 31, 37) is deterministic up to n < 3.3e24, easily covering anything
# representable in f64-safe ints.
#
# Input lines:
#   "POW <base> <exp> <mod>"   print "<base>^<exp> mod <mod> = <result>"
#   "PRIME <n>"                print "PRIME n -> YES" or "PRIME n -> NO"
# All inputs must satisfy mod / n < 2^26 so intermediate (a*a) % m fits in f64
# without precision loss.

function mul_mod(a, b, m) { return (a * b) % m }

function pow_mod(base, ex, m,   r) {
  r = 1
  base = base % m
  while (ex > 0) {
    if (ex % 2 == 1) r = mul_mod(r, base, m)
    base = mul_mod(base, base, m)
    ex = int(ex / 2)
  }
  return r
}

function miller_witness(a, n, d, s,   x, r) {
  x = pow_mod(a, d, n)
  if (x == 1 || x == n - 1) return 1
  for (r = 1; r < s; r++) {
    x = mul_mod(x, x, n)
    if (x == n - 1) return 1
  }
  return 0
}

function is_prime(n,   d, s, i, a) {
  if (n < 2) return 0
  for (i = 2; i <= 37; i++) {
    if (n == i) return 1
    if (n % i == 0) return 0
  }
  d = n - 1; s = 0
  while (d % 2 == 0) { d = int(d / 2); s++ }
  split("2 3 5 7 11 13 17 19 23 29 31 37", witnesses, " ")
  for (i = 1; i in witnesses; i++) {
    a = witnesses[i] + 0
    if (a >= n) continue
    if (!miller_witness(a, n, d, s)) return 0
  }
  return 1
}

$1 == "POW" {
  printf "%d^%d mod %d = %d\n", $2 + 0, $3 + 0, $4 + 0, pow_mod($2 + 0, $3 + 0, $4 + 0)
  next
}
$1 == "PRIME" {
  printf "PRIME %d -> %s\n", $2 + 0, is_prime($2 + 0) ? "YES" : "NO"
  next
}
