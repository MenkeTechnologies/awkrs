# LRU cache: O(1) get + put via a doubly linked list (head = most-recent,
# tail = least-recent) over arrays, plus a hash map from key -> node id.
# Sentinel nodes at id 0 (head) and 1 (tail) so we never special-case ends.
#
# Input lines:
#   "CAP <n>"        set capacity (must come first, before any op)
#   "PUT <k> <v>"    insert / update; evicts LRU on overflow
#   "GET <k>"        prints "GET k -> v" or "GET k -> MISS"
#   "DUMP"           prints "[ k1=v1, k2=v2, ... ]" head→tail
# CAP must be > 0.

function unlink(n) {
  nxt[prev[n]] = nxt[n]
  prev[nxt[n]] = prev[n]
}
function link_after(n, target) {
  nxt[n] = nxt[target]; prev[n] = target
  prev[nxt[target]] = n
  nxt[target] = n
}
function touch(n) { unlink(n); link_after(n, 0) }

function lru_init() {
  nxt[0] = 1; prev[0] = 1   # head's next is tail
  nxt[1] = 0; prev[1] = 0   # tail's prev is head — only sentinels in the list
  free_id = 2
  size = 0
}

function lru_get(k) {
  if (!(k in id_of)) { printf "GET %s -> MISS\n", k; return }
  n = id_of[k]
  touch(n)
  printf "GET %s -> %s\n", k, val[n]
}

function lru_put(k, v,   n, evicted) {
  if (k in id_of) {
    n = id_of[k]
    val[n] = v
    touch(n)
    return
  }
  n = free_id++
  val[n] = v; key_of[n] = k; id_of[k] = n
  link_after(n, 0)
  size++
  if (size > CAP) {
    evicted = prev[1]      # tail's prev = LRU
    unlink(evicted)
    delete id_of[key_of[evicted]]
    delete val[evicted]; delete key_of[evicted]
    size--
  }
}

function dump(   n, out, k, v) {
  out = "[ "
  sep = ""
  n = nxt[0]
  while (n != 1) {
    out = out sep key_of[n] "=" val[n]
    sep = ", "
    n = nxt[n]
  }
  out = out " ]"
  print out
}

NR == 1 && $1 == "CAP" { CAP = $2 + 0; lru_init(); next }

$1 == "PUT"  { lru_put($2, $3); next }
$1 == "GET"  { lru_get($2);     next }
$1 == "DUMP" { dump();          next }
