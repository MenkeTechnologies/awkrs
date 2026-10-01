# POSIX bracket expressions: `[` is an ordinary member, a `]` first in the list
# (after an optional `^`) is a member, and `/` inside a bracket does not end a
# regexp literal. gawk's operators: \y (word boundary), \` and \' (string
# start and end).
BEGIN {
  s = "x[1] = y[22];"; n = gsub(/[][]/, "", s); print n, s
  t = "a[b"; print gsub(/[[]/, "<", t), t
  print ("x]" ~ /^[]x]+$/), ("]" ~ /[^]]/), ("a" ~ /[^]]/)
  print ("a/b" ~ /a[/]b/), ("/" ~ /[]/]/)
  print split("p[q]r", parts, /[][]/), parts[1] parts[2] parts[3]
  u = "a&b~c"; print gsub(/[&~]/, "+", u), u
  v = "foo bar"; print gsub(/\y/, "|", v), v
  w = "abc"; print gsub(/\`a/, "X", w), gsub(/c\'/, "Y", w), w
}
