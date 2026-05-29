# Min-heap priority queue → heapsort the input numbers ascending.
# hp[] is the 1-indexed binary heap; hn = current size.

function hpush(v,   i, t) {
  hp[++hn] = v
  i = hn
  while (i > 1) {
    t = int(i / 2)
    if (hp[t] > hp[i]) {
      v = hp[t]; hp[t] = hp[i]; hp[i] = v
      i = t
    } else { break }
  }
}

function hpop(   r, i, l, rt, sm, t) {
  r = hp[1]
  hp[1] = hp[hn]
  delete hp[hn]
  hn--
  i = 1
  while (1) {
    l = 2 * i; rt = l + 1; sm = i
    if (l  <= hn && hp[l]  < hp[sm]) sm = l
    if (rt <= hn && hp[rt] < hp[sm]) sm = rt
    if (sm == i) break
    t = hp[i]; hp[i] = hp[sm]; hp[sm] = t
    i = sm
  }
  return r
}

BEGIN { hn = 0 }

{ hpush($1 + 0) }

END {
  sep = ""
  while (hn > 0) {
    printf "%s%d", sep, hpop()
    sep = " "
  }
  print ""
}
