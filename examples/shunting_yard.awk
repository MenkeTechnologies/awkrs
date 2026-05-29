# Dijkstra's shunting-yard: convert infix arithmetic to postfix (RPN), then
# evaluate the RPN. Operators: + - * / ^ unary-minus; parens; integer or
# decimal literals.
# ^ is right-associative; * / share precedence with left-assoc; + - share
# precedence with left-assoc.
#
# Input: one expression per line.
# Output: "<expr>  RPN: <postfix>  =  <value>"

function is_op(t)  { return (t == "+" || t == "-" || t == "*" || t == "/" || t == "^" || t == "u-") }
function prec(t)   { return (t == "u-") ? 4 : (t == "^" ? 4 : (t == "*" || t == "/" ? 3 : 2)) }
function rassoc(t) { return (t == "^" || t == "u-") }

function tokenize(s,   n, i, c, num, prev_was_op) {
  delete tk
  tn = 0
  n = length(s); i = 1
  prev_was_op = 1
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == " " || c == "\t") { i++; continue }
    if (c ~ /[0-9.]/) {
      num = ""
      while (i <= n && substr(s, i, 1) ~ /[0-9.]/) { num = num substr(s, i, 1); i++ }
      tk[++tn] = num
      prev_was_op = 0
      continue
    }
    if (c == "(") { tk[++tn] = "("; prev_was_op = 1; i++; continue }
    if (c == ")") { tk[++tn] = ")"; prev_was_op = 0; i++; continue }
    if (c == "-" && prev_was_op) { tk[++tn] = "u-"; prev_was_op = 1; i++; continue }
    if (c ~ /[+\-*\/^]/) { tk[++tn] = c; prev_was_op = 1; i++; continue }
    printf "tokenize: bad char %s at %d\n", c, i
    return 0
  }
  return 1
}

function to_rpn(   i, t, top, out_n, sep) {
  out_n = 0; sp = 0
  for (i = 1; i <= tn; i++) {
    t = tk[i]
    if (t ~ /^[0-9.]+$/) { out[++out_n] = t; continue }
    if (t == "(")   { stk[++sp] = t; continue }
    if (t == ")") {
      while (sp > 0 && stk[sp] != "(") { out[++out_n] = stk[sp--] }
      if (sp == 0) { print "mismatched )"; return 0 }
      sp--
      continue
    }
    if (is_op(t)) {
      while (sp > 0 && is_op(stk[sp]) \
          && (prec(stk[sp]) > prec(t) \
              || (prec(stk[sp]) == prec(t) && !rassoc(t)))) {
        out[++out_n] = stk[sp--]
      }
      stk[++sp] = t
      continue
    }
  }
  while (sp > 0) {
    if (stk[sp] == "(") { print "mismatched ("; return 0 }
    out[++out_n] = stk[sp--]
  }
  rpn_n = out_n
  return 1
}

function rpn_string(   i, s, sep) {
  s = ""; sep = ""
  for (i = 1; i <= rpn_n; i++) { s = s sep out[i]; sep = " " }
  return s
}

function eval_rpn(   i, t, a, b, vsp) {
  vsp = 0
  for (i = 1; i <= rpn_n; i++) {
    t = out[i]
    if (t ~ /^[0-9.]+$/) { vs[++vsp] = t + 0; continue }
    if (t == "u-") { vs[vsp] = -vs[vsp]; continue }
    b = vs[vsp--]; a = vs[vsp--]
    if      (t == "+") vs[++vsp] = a + b
    else if (t == "-") vs[++vsp] = a - b
    else if (t == "*") vs[++vsp] = a * b
    else if (t == "/") vs[++vsp] = a / b
    else if (t == "^") vs[++vsp] = a ^ b
  }
  return vs[1]
}

NF == 0 { next }
{
  expr = $0
  if (!tokenize(expr)) next
  if (!to_rpn()) next
  v = eval_rpn()
  if (v == int(v)) printf "%s  RPN: %s  =  %d\n", expr, rpn_string(), v
  else             printf "%s  RPN: %s  =  %g\n", expr, rpn_string(), v
}
