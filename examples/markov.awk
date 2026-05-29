# Bigram Markov-chain analytics over a text corpus.
# 1. Tokenize each input line into lowercase a-z words.
# 2. Build bg[prev, curr] count.
# 3. For each starting word, list the top-3 follow-on words by count
#    (lexicographic tie-break) — deterministic output, no rand() needed.
# 4. Also deterministically walk a 12-step chain starting from "the":
#    at each step, pick the next word with highest count; lex tie-break.

function bigrams_for_line(line,   n, i) {
  gsub(/[^a-zA-Z]+/, " ", line)
  line = tolower(line)
  n = split(line, w, " ")
  for (i = 1; i < n; i++) {
    if (w[i] == "" || w[i+1] == "") continue
    bg[w[i], w[i+1]]++
    starts[w[i]]++
  }
}

function top_next(prev,   k, a, b, top1, top2, top3, c1, c2, c3) {
  top1 = ""; top2 = ""; top3 = ""
  c1 = c2 = c3 = -1
  for (k in bg) {
    split(k, parts, SUBSEP)
    if (parts[1] != prev) continue
    a = parts[2]; b = bg[k]
    # insert into top-3 sorted by (count desc, name asc)
    if (b > c1 || (b == c1 && a < top1)) {
      c3 = c2; top3 = top2
      c2 = c1; top2 = top1
      c1 = b;  top1 = a
    } else if (b > c2 || (b == c2 && a < top2)) {
      c3 = c2; top3 = top2
      c2 = b;  top2 = a
    } else if (b > c3 || (b == c3 && a < top3)) {
      c3 = b;  top3 = a
    }
  }
  out = top1 "(" c1 ")"
  if (top2 != "") out = out " " top2 "(" c2 ")"
  if (top3 != "") out = out " " top3 "(" c3 ")"
  return out
}

function walk(seed, steps,   path, cur, i) {
  cur = seed; path = cur
  for (i = 1; i <= steps; i++) {
    # find best next via top_next-style scan (just top1)
    best = ""; bcnt = -1
    for (k in bg) {
      split(k, parts, SUBSEP)
      if (parts[1] != cur) continue
      cv = bg[k]
      if (cv > bcnt || (cv == bcnt && parts[2] < best)) { bcnt = cv; best = parts[2] }
    }
    if (best == "") break
    path = path " " best
    cur = best
  }
  return path
}

{ bigrams_for_line($0) }

END {
  print "[top continuations]"
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (s in starts) {
    if (starts[s] < 2) continue
    printf "  %s -> %s\n", s, top_next(s)
  }
  print "[walk from \"the\"]"
  print "  " walk("the", 12)
}
