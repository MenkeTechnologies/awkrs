# Trie over a 2D associative array via SUBSEP.
# Input lines:
#   ADD <word>      insert <word> into the trie
#   FIND <word>     report HIT if exact word is present, miss otherwise
#   PFX <prefix>    report COUNT n  (#words sharing this prefix)
# Trie state:
#   edge[node, char] = child node id (1-based; 0 = root)
#   term[node]       = 1 when node is the end of an inserted word
#   subcnt[node]     = number of inserted words in this node's subtree

function tr_add(s,   i, p, c, k) {
  p = 0
  subcnt[p]++
  for (i = 1; i <= length(s); i++) {
    c = substr(s, i, 1)
    k = p SUBSEP c
    if (!(k in edge)) { edge[k] = ++tn; subcnt[tn] = 0 }
    p = edge[k]
    subcnt[p]++
  }
  term[p] = 1
}

function tr_find(s,   i, p, c, k) {
  p = 0
  for (i = 1; i <= length(s); i++) {
    c = substr(s, i, 1); k = p SUBSEP c
    if (!(k in edge)) return 0
    p = edge[k]
  }
  return (p in term) ? 1 : 0
}

function tr_pfxcount(s,   i, p, c, k) {
  p = 0
  for (i = 1; i <= length(s); i++) {
    c = substr(s, i, 1); k = p SUBSEP c
    if (!(k in edge)) return 0
    p = edge[k]
  }
  return subcnt[p]
}

BEGIN { tn = 0 }

$1 == "ADD"  { tr_add($2);                                          next }
$1 == "FIND" { printf "%s -> %s\n", $2, (tr_find($2) ? "HIT" : "miss"); next }
$1 == "PFX"  { printf "%s -> COUNT %d\n", $2, tr_pfxcount($2);      next }
