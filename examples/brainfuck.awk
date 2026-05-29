# Brainfuck interpreter.
# Input format:
#   line 1     : the Brainfuck program (no newlines inside it)
#   line 2 ... : program stdin (joined w/o newlines; each char fed to ',' in turn)
#
# Tape is unbounded sparse via associative array; cells modulo 256.

function bf_run(src, prog_in,   ip, dp, n, c, j, sp, ii, out) {
  n = length(src)
  sp = 0

  # Pre-compute matching brackets so loops are O(1).
  for (ip = 1; ip <= n; ip++) {
    c = substr(src, ip, 1)
    if (c == "[") { stack[++sp] = ip; continue }
    if (c == "]") {
      if (sp == 0) return "ERR: unbalanced ]"
      j = stack[sp--]
      jmp_fwd[j]  = ip
      jmp_back[ip] = j
    }
  }
  if (sp != 0) return "ERR: unbalanced ["

  ip = 1; dp = 1; ii = 1; out = ""
  while (ip <= n) {
    c = substr(src, ip, 1)
    if (c == ">") { dp++;                                     ip++; continue }
    if (c == "<") { dp--;                                     ip++; continue }
    if (c == "+") { tape[dp] = (tape[dp] + 1)       % 256;    ip++; continue }
    if (c == "-") { tape[dp] = (tape[dp] - 1 + 256) % 256;    ip++; continue }
    if (c == ".") { out = out sprintf("%c", tape[dp]);        ip++; continue }
    if (c == ",") {
      if (ii <= length(prog_in)) {
        tape[dp] = ord(substr(prog_in, ii, 1)); ii++
      } else { tape[dp] = 0 }
      ip++; continue
    }
    if (c == "[") { if (tape[dp] == 0) ip = jmp_fwd[ip];      ip++; continue }
    if (c == "]") { if (tape[dp] != 0) ip = jmp_back[ip];     ip++; continue }
    ip++
  }
  return out
}

function ord(ch,   i) {
  if (!_ord_init) {
    for (i = 0; i < 256; i++) _ord[sprintf("%c", i)] = i
    _ord_init = 1
  }
  return _ord[ch]
}

NR == 1 { prog = $0; next }
NR == 2 { stdin_buf = $0; next }

END {
  if (NR < 2) stdin_buf = ""
  printf "%s\n", bf_run(prog, stdin_buf)
}
