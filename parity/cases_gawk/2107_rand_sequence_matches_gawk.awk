# gawk's rand() is BSD random(3) (TYPE_4 with gawk's 512-entry shuffle) behind
# do_rand, so a seeded sequence is reproducible against gawk.
BEGIN {
  printf "%.6f %.6f %.6f\n", rand(), rand(), rand()
  print srand(42); for (i = 0; i < 5; i++) printf "%.10f ", rand(); print ""
  print srand(7); srand(0); printf "%.8f\n", rand()
  srand(123456789); for (i = 0; i < 1000; i++) s += rand(); printf "%.9f\n", s
  srand(5); for (i = 0; i < 10; i++) printf "%d ", int(rand() * 100); print ""
}
