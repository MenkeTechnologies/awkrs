# Aho-Corasick multi-pattern string matching: build a goto/fail/output trie
# from a list of patterns, then scan a text linearly, emitting every match.
#
# Input lines:
#   "PAT <p>"           add pattern p to the dictionary
#   "BUILD"             freeze the dictionary and compute fail links
#   "TXT <s>"           scan s; print "TXT s -> pat@pos pat@pos ..." (1-based
#                       end-position of each match) or "TXT s -> NONE"
# Multiple PAT lines may precede BUILD; multiple TXT lines may follow.
# Trie:
#   goto[node, char] = child node id (0 means follow fail link)
#   fail[node]       = suffix-failure link
#   out[node]        = space-separated patterns ending at this node

function tr_add(p,   i, n, c, cur, nxt) {
  n = length(p)
  cur = 0
  for (i = 1; i <= n; i++) {
    c = substr(p, i, 1)
    if (!((cur SUBSEP c) in goto_)) {
      goto_[cur, c] = ++tn
      fail[tn] = 0
    }
    cur = goto_[cur, c]
  }
  out[cur] = (out[cur] == "" ? p : out[cur] " " p)
}

function tr_build(   q_head, q_tail, c, u, v, r, k) {
  delete q
  q_head = 1; q_tail = 0
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (k in goto_) {
    split(k, parts, SUBSEP)
    if (parts[1] != 0) continue
    c = parts[2]
    v = goto_[k]
    fail[v] = 0
    q_tail++; q[q_tail] = v
  }
  while (q_head <= q_tail) {
    u = q[q_head]; q_head++
    # For each child (u --c--> v): bfs and link.
    delete kids
    for (k in goto_) {
      split(k, parts, SUBSEP)
      if (parts[1] != u) continue
      kids[parts[2]] = goto_[k]
    }
    for (c in kids) {
      v = kids[c]
      r = fail[u]
      while (r != 0 && !((r SUBSEP c) in goto_)) r = fail[r]
      fail[v] = ((r SUBSEP c) in goto_ && goto_[r, c] != v) ? goto_[r, c] : 0
      if (out[fail[v]] != "") out[v] = (out[v] == "" ? out[fail[v]] : out[v] " " out[fail[v]])
      q_tail++; q[q_tail] = v
    }
  }
}

function ac_scan(s,   i, n, c, cur, hits, name, sep, nx) {
  hits = ""; sep = ""
  cur = 0
  n = length(s)
  for (i = 1; i <= n; i++) {
    c = substr(s, i, 1)
    while (cur != 0 && !((cur SUBSEP c) in goto_)) cur = fail[cur]
    if ((cur SUBSEP c) in goto_) cur = goto_[cur, c]
    if (out[cur] != "") {
      nm = out[cur]
      split(nm, names, " ")
      for (k = 1; k in names; k++) {
        hits = hits sep names[k] "@" i
        sep = " "
      }
    }
  }
  return hits == "" ? "NONE" : hits
}

$1 == "PAT"   { tr_add(substr($0, 5)); next }
$1 == "BUILD" { tr_build(); next }
$1 == "TXT"   { txt = substr($0, 5); printf "TXT %s -> %s\n", txt, ac_scan(txt); next }
