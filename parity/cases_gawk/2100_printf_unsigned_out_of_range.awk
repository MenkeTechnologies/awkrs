# gawk printf %o %u %x %X: the value goes through uintmax_t (intmax_t when
# negative) and prints only if it survives the round trip; otherwise it falls
# back to %g with the same flags, width and precision. Operands of 2^64 and
# beyond are left out: gawk's C cast of them is undefined (aarch64 saturates,
# x86-64 does not).
BEGIN {
  printf "%x %X %o %u\n", 2^64 - 2048, 2^63, 2^63, 2^64 - 4096
  printf "%x %x %u\n", 2^63 + 2^62, -1, -2^63
  printf "%x|%x|%+x|% o|%5.3x|%-12x|%#x|%u\n", 1e30, -1e30, 1e30, 1e30, 1e30, 1e30, 1e30, 2^65
}
