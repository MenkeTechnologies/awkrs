# gawk and/or/xor/lshift/rshift/compl: (uintmax_t) operands, 64-bit unsigned
# arithmetic, adjust_uint narrowing of the result.
# The record rule runs on fusevm's tiers, the BEGIN loops through its JIT.
# Operands of 2^64 and beyond, and shift counts of 64 or more, are left out:
# gawk's C cast of the first is undefined (aarch64 saturates, x86-64 does
# not), and gawk 5.2.1 on x86-64 (CI) shifts by the count modulo 64 where
# gawk 5.4.1 yields 0.
BEGIN {
  x = 2^54
  print or(x, 1), and(x + 3, 7), xor(2^60, 1), lshift(1, 63), rshift(2^60, 3)
  print lshift(3, 62), rshift(5, 2), lshift(2^52, 3), compl(2^63), or(2^63 + 2^11, 1)
  print and(2^60 + 2^12 + 1, 2^61 + 2^60 + 1, 2^60 + 2^12)
  for (i = 0; i < 70; i += 7) s = or(s, lshift(1, i))
  for (i = 54; i < 64; i++) t += compl(lshift(1, i))
  print s, t
}
{ print or($1, 1), lshift($1, $2), rshift($1, $2), compl($2), and($1, $2, 1), xor($1, $2) }
