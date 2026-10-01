# gawk and/or/xor/lshift/rshift/compl: (uintmax_t) operands, 64-bit unsigned
# arithmetic, adjust_uint narrowing of the result, a shift count >= 64 is 0.
# The record rule runs on fusevm's tiers, the BEGIN loops through its JIT.
# Operands of 2^64 and beyond are left out: gawk's C cast of them is
# undefined (aarch64 saturates, x86-64 does not).
BEGIN {
  x = 2^54
  print or(x, 1), and(x + 3, 7), xor(2^60, 1), lshift(1, 70), rshift(2^60, 3)
  print lshift(3, 64), rshift(5, 64), lshift(2^52, 3), compl(2^63), or(2^63 + 2^11, 1)
  print and(2^60 + 2^12 + 1, 2^61 + 2^60 + 1, 2^60 + 2^12)
  for (i = 0; i < 70; i += 7) s = or(s, lshift(1, i))
  for (i = 60; i < 70; i++) t += compl(lshift(1, i))
  print s, t
}
{ print or($1, 1), lshift($1, $2), rshift($1, $2), compl($2), and($1, $2, 1), xor($1, $2) }
