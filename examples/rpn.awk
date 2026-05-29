# Reverse Polish (postfix) calculator.
# Input: one expression per line, space-separated tokens. Operators: + - * / % ^
# negate (unary), dup, swap, drop. Numbers push onto the stack.
# Output: "<expr> = <top-of-stack>" after the line, or "<expr> = ERR ..." on
# malformed input.

function err(msg, line) { printf "%s = ERR %s\n", line, msg }

{
  sp = 0
  delete st
  ok = 1
  for (i = 1; i <= NF; i++) {
    t = $i
    if (t ~ /^-?[0-9]+(\.[0-9]+)?$/) { st[++sp] = t + 0; continue }
    if (t == "+" || t == "-" || t == "*" || t == "/" || t == "%" || t == "^") {
      if (sp < 2) { err("underflow at "t, $0); ok = 0; break }
      b = st[sp--]; a = st[sp--]
      if      (t == "+") st[++sp] = a + b
      if      (t == "-") st[++sp] = a - b
      if      (t == "*") st[++sp] = a * b
      if      (t == "/") { if (b == 0) { err("div0", $0); ok = 0; break } st[++sp] = a / b }
      if      (t == "%") { if (b == 0) { err("mod0", $0); ok = 0; break } st[++sp] = a % b }
      if      (t == "^") st[++sp] = a ^ b
      continue
    }
    if (t == "neg")  { if (sp < 1) { err("underflow neg", $0); ok = 0; break } st[sp] = -st[sp]; continue }
    if (t == "dup")  { if (sp < 1) { err("underflow dup", $0); ok = 0; break } st[sp + 1] = st[sp]; sp++; continue }
    if (t == "swap") { if (sp < 2) { err("underflow swap", $0); ok = 0; break } tmp = st[sp]; st[sp] = st[sp-1]; st[sp-1] = tmp; continue }
    if (t == "drop") { if (sp < 1) { err("underflow drop", $0); ok = 0; break } delete st[sp]; sp--; continue }
    err("unknown token "t, $0); ok = 0; break
  }
  if (!ok) next
  if (sp != 1) { err("stack depth "sp" at end", $0); next }
  v = st[1]
  if (v == int(v)) printf "%s = %d\n", $0, v
  if (v != int(v)) printf "%s = %g\n", $0, v
}
