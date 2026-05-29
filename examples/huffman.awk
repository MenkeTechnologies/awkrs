# Huffman coding — build the prefix-code tree from character frequencies in
# the input text, emit each character's code, then encode + round-trip decode.
# Input: a single line of text.
# Output:
#   the code table (char freq code), one row per distinct char, sorted by char;
#   ENC: <bitstring>
#   DEC: <recovered text>
#   COMPRESSION: <orig_bits>/<enc_bits>
#
# Tie-break for the priority queue: (freq asc, then lex-min representative
# char asc) so output is deterministic regardless of build order.
# Tree stored as parallel arrays:
#   ch[i]   leaf char (empty for internal nodes)
#   fr[i]   frequency
#   lo[i]   leftmost char in subtree (tie-break)
#   lc[i]   left child id
#   rc[i]   right child id

function extract_min(   best, key, i) {
  best = 0; key = ""
  for (i in active) {
    k = sprintf("%012d %s %d", fr[i], lo[i], i)
    if (best == 0 || k < key) { best = i; key = k }
  }
  delete active[best]
  return best
}

function assign_codes(node, prefix) {
  if (lc[node] == 0 && rc[node] == 0) {
    code[ch[node]] = (prefix == "" ? "0" : prefix)   # single-char text edge case
    return
  }
  if (lc[node] != 0) assign_codes(lc[node], prefix "0")
  if (rc[node] != 0) assign_codes(rc[node], prefix "1")
}

NR == 1 {
  text = $0
  for (i = 1; i <= length(text); i++) {
    c = substr(text, i, 1)
    freq[c]++
  }

  nn = 0
  for (c in freq) {
    nn++
    ch[nn] = c; fr[nn] = freq[c]; lo[nn] = c
    lc[nn] = 0; rc[nn] = 0
    active[nn] = 1
  }

  while (1) {
    cnt = 0
    for (i in active) cnt++
    if (cnt <= 1) break
    a = extract_min()
    b = extract_min()
    nn++
    ch[nn] = ""; fr[nn] = fr[a] + fr[b]
    lo[nn] = (lo[a] < lo[b]) ? lo[a] : lo[b]
    lc[nn] = a; rc[nn] = b
    active[nn] = 1
  }

  for (i in active) root = i
  assign_codes(root, "")

  # Emit code table sorted by char.
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (c in freq) printf "char=%s  freq=%d  code=%s\n", c, freq[c], code[c]

  # Encode.
  enc = ""
  for (i = 1; i <= length(text); i++) enc = enc code[substr(text, i, 1)]
  printf "ENC: %s\n", enc

  # Decode by walking the tree.
  dec = ""; cur = root
  for (i = 1; i <= length(enc); i++) {
    bit = substr(enc, i, 1)
    cur = (bit == "0") ? lc[cur] : rc[cur]
    if (lc[cur] == 0 && rc[cur] == 0) {
      dec = dec ch[cur]
      cur = root
    }
  }
  printf "DEC: %s\n", dec

  printf "COMPRESSION: %d/%d\n", length(text) * 8, length(enc)
  exit 0
}
