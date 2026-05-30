# Horner's method for polynomial evaluation, plus the synthetic-division
# variant that simultaneously gives p(x) and the quotient polynomial after
# dividing by (x - x0). One line at a time:
#
#   "EVAL a_n a_{n-1} ... a_0  AT x"
#     -> "<poly> at x=<x> = <p(x)>"
#   "DERIV a_n a_{n-1} ... a_0  AT x"
#     -> "<poly>'(x=<x>) = <p'(x)>"   (also via Horner)
#   "DIV   a_n a_{n-1} ... a_0  AT x0"
#     -> "<poly> / (x - x0) = <quot>  rem <p(x0)>"

function horner(coef, n, x,   i, r) {
  r = 0
  for (i = 1; i <= n; i++) r = r * x + coef[i]
  return r
}

function horner_div(coef, n, x0,   i, r, out) {
  delete quot
  r = coef[1]
  quot[1] = r
  for (i = 2; i <= n; i++) {
    r = r * x0 + coef[i]
    quot[i] = r
  }
  # quot[n] is p(x0); quot[1..n-1] are the coefficients of the quotient poly.
  return r
}

function poly_string(coef, n,   i, s, sign, c, exp_) {
  s = ""
  for (i = 1; i <= n; i++) {
    c = coef[i]; exp_ = n - i
    if (c == 0) continue
    if (s == "") {
      s = c "x^" exp_
    } else {
      sign = (c >= 0) ? " + " : " - "
      s = s sign (c < 0 ? -c : c) "x^" exp_
    }
  }
  if (s == "") s = "0"
  return s
}

function quot_string(   i, s, c) {
  s = ""
  for (i = 1; i < cur_n; i++) {
    c = quot[i]
    if (c == 0 && s == "") continue
    if (s == "") s = c "x^" (cur_n - 1 - i)
    else {
      s = s ((c >= 0) ? " + " : " - ") (c < 0 ? -c : c) "x^" (cur_n - 1 - i)
    }
  }
  if (s == "") s = "0"
  return s
}

NF == 0 { next }

{
  cmd = $1
  # Find the "AT" token to split coefficients from x.
  at_idx = 0
  for (i = 1; i <= NF; i++) if ($i == "AT") { at_idx = i; break }
  if (at_idx == 0) { print "missing AT"; next }
  n = at_idx - 2   # number of coefficients
  for (i = 1; i <= n; i++) coef[i] = $(i + 1) + 0
  x = $(at_idx + 1) + 0
  cur_n = n

  if (cmd == "EVAL") {
    v = horner(coef, n, x)
    printf "%s at x=%g = %g\n", poly_string(coef, n), x, v
    next
  }
  if (cmd == "DERIV") {
    delete dcoef
    for (i = 1; i < n; i++) dcoef[i] = coef[i] * (n - i)
    v = horner(dcoef, n - 1, x)
    printf "%s'(x=%g) = %g\n", poly_string(coef, n), x, v
    next
  }
  if (cmd == "DIV") {
    r = horner_div(coef, n, x)
    printf "%s / (x - %g) = %s  rem %g\n", poly_string(coef, n), x, quot_string(), r
    next
  }
}
