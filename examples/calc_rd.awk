# Recursive-descent arithmetic evaluator.
# Supports + - * / % ^ unary-minus parentheses and integer/decimal literals.
# ^ is right-associative; * / % share precedence and are left-associative;
# + - share precedence and are left-associative.
#
# Input: one expression per line. Output: "expr = value" (one per line).
# Globals between calls:
#   tk[1..ntk]  token array
#   tp          current token index (1-based)

function tokenize(s,   i, n, c, num) {
  ntk = 0
  delete tk
  n = length(s)
  i = 1
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == " " || c == "\t") { i++; continue }
    if (c ~ /[0-9.]/) {
      num = ""
      while (i <= n && substr(s, i, 1) ~ /[0-9.]/) { num = num substr(s, i, 1); i++ }
      tk[++ntk] = num
      continue
    }
    if (c ~ /[+\-*\/%^()]/) { tk[++ntk] = c; i++; continue }
    printf "tokenize: unexpected '%s' at %d\n", c, i
    return 0
  }
  tk[++ntk] = "$"
  return 1
}

function peek() { return tk[tp] }
function eat(t,   x) { x = tk[tp]; if (x != t) { printf "parse: want %s got %s\n", t, x; exit 1 } tp++; return x }

function p_atom(   v) {
  if (peek() == "(") { eat("("); v = p_expr(); eat(")"); return v }
  v = tk[tp] + 0; tp++; return v
}

function p_power(   a, b) {
  a = p_atom()
  if (peek() == "^") { eat("^"); b = p_unary(); return a ^ b }
  return a
}

function p_unary(   v) {
  if (peek() == "-") { eat("-"); return -p_unary() }
  if (peek() == "+") { eat("+"); return  p_unary() }
  return p_power()
}

function p_muldiv(   a, op, b) {
  a = p_unary()
  while (peek() == "*" || peek() == "/" || peek() == "%") {
    op = peek(); eat(op); b = p_unary()
    if      (op == "*") a = a * b
    else if (op == "/") a = a / b
    else                a = a % b
  }
  return a
}

function p_expr(   a, op, b) {
  a = p_muldiv()
  while (peek() == "+" || peek() == "-") {
    op = peek(); eat(op); b = p_muldiv()
    if (op == "+") a = a + b; else a = a - b
  }
  return a
}

NF == 0 { next }
{
  if (!tokenize($0)) next
  tp = 1
  v = p_expr()
  if (peek() != "$") { printf "%s = parse error (trailing token %s)\n", $0, peek(); next }
  if (v == int(v)) printf "%s = %d\n", $0, v
  else             printf "%s = %g\n", $0, v
}
