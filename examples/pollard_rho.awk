# Pollard's rho integer factorization for small / moderate inputs.
# Input: one positive integer per line.
# Output: "<n> = p1 * p2 * ..."  (ascending prime factors with multiplicity)
#         "<n> = 1"               for n == 1
#         "<n> = INVALID"         for n <= 0
#
# Strategy:
#   1. Strip small primes by trial division up to 100.
#   2. While remaining factor > 1:
#        - if Miller-Rabin says prime, emit it
#        - else split via Pollard's rho with Brent's cycle detection
# Bound: n / max prime factor must fit in f64 (< 2^53) for the integer
# arithmetic to stay exact. Examples below are well under that.

function abs1(x) { return (x < 0) ? -x : x }
function mul_mod(a, b, m) { return (a * b) % m }

function pow_mod(base, ex, m,   r) {
  r = 1; base = base % m
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
  if (n == 2 || n == 3 || n == 5 || n == 7) return 1
  if (n % 2 == 0 || n % 3 == 0 || n % 5 == 0 || n % 7 == 0) return 0
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

function gcd(a, b,   t) { while (b) { t = b; b = a % b; a = t } return a }

function brent_split(n,   x, y, c, m, g, q, ys, k, r, i, lim) {
  # Brent's cycle detection variant of Pollard rho.
  c = 1
  while (1) {
    y = 2; r = 1; q = 1; g = 1; m = 64
    while (g == 1) {
      x = y
      for (i = 1; i <= r; i++) y = (mul_mod(y, y, n) + c) % n
      k = 0
      while (k < r && g == 1) {
        ys = y
        lim = (m < r - k) ? m : (r - k)
        for (i = 1; i <= lim; i++) {
          y = (mul_mod(y, y, n) + c) % n
          q = mul_mod(q, abs1(x - y), n)
        }
        g = gcd(q, n)
        k += m
      }
      r = r * 2
    }
    if (g == n) {
      g = 1
      while (g == 1) {
        ys = (mul_mod(ys, ys, n) + c) % n
        g = gcd(abs1(x - ys), n)
      }
    }
    if (g != n) return g
    c++
    if (c > 30) return n   # give up; treat as prime
  }
}

function factor(n) {
  if (n <= 0) { factors_out = "INVALID"; return }
  if (n == 1) { factors_out = "1"; return }

  delete primes_found
  # trial division by small primes
  split("2 3 5 7 11 13 17 19 23 29 31 37 41 43 47 53 59 61 67 71 73 79 83 89 97", small, " ")
  for (i = 1; i in small; i++) {
    p = small[i] + 0
    while (n % p == 0) { primes_found[++pf_n] = p; n = int(n / p) }
  }

  # remaining factor via Pollard rho / Miller-Rabin
  delete stack
  sp = 0
  if (n > 1) stack[++sp] = n
  while (sp > 0) {
    v = stack[sp]; sp--
    if (v == 1) continue
    if (is_prime(v)) { primes_found[++pf_n] = v; continue }
    d = brent_split(v)
    stack[++sp] = d
    stack[++sp] = int(v / d)
  }

  # sort ascending via insertion sort
  for (i = 2; i <= pf_n; i++) {
    k = primes_found[i]; j = i - 1
    while (j >= 1 && primes_found[j] > k) { primes_found[j + 1] = primes_found[j]; j-- }
    primes_found[j + 1] = k
  }

  out = ""
  for (i = 1; i <= pf_n; i++) out = out (i == 1 ? "" : " * ") primes_found[i]
  if (out == "") out = "1"
  factors_out = out
  pf_n = 0
}

NF == 0 { next }
{
  n_in = $1 + 0
  pf_n = 0
  factor(n_in)
  printf "%d = %s\n", n_in, factors_out
}
