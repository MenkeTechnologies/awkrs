# Tiny regex engine — recursive matcher supporting:
#   ^   start anchor
#   $   end anchor
#   .   any single char
#   *   zero-or-more of previous atom
#   +   one-or-more of previous atom
#   ?   zero-or-one of previous atom
#   [..] character class (no ranges, no negation — keep it tiny)
#   \X  literal X (escape)
#
# Input lines: "<pattern> <text>". Output: "<pattern>  <text>  MATCH" or NOMATCH.
# Implementation: classic K&R "Software Tools" recursive matcher, extended to
# bracket classes and `+`/`?` quantifiers. NOT awk's own regex — this is a
# scratch matcher written *in* awk for demonstration.

function atom_match(atom, ch,   k, body) {
  if (atom == ".") return 1
  if (substr(atom, 1, 1) == "[") {
    body = substr(atom, 2, length(atom) - 2)
    for (k = 1; k <= length(body); k++) {
      if (substr(body, k, 1) == ch) return 1
    }
    return 0
  }
  return (atom == ch) ? 1 : 0
}

# Return the byte length the next atom occupies in the pattern starting at i.
function atom_len(pat, i,   c, j, n) {
  c = substr(pat, i, 1)
  if (c == "\\") return 2
  if (c == "[") {
    n = length(pat); j = i + 1
    while (j <= n && substr(pat, j, 1) != "]") j++
    if (j > n) return 1  # unterminated; treat as literal '['
    return j - i + 1
  }
  return 1
}

# Read atom at pat[i..]; return its match-key form: "." / "[abc]" / single char.
function atom_at(pat, i,   c, l) {
  c = substr(pat, i, 1)
  l = atom_len(pat, i)
  if (c == "\\") return substr(pat, i + 1, 1)
  return substr(pat, i, l)
}

# Match `atom` followed by `rest` against text starting at pos t. Used for `*`.
function match_star(atom, rest, text, t,   tl) {
  tl = length(text)
  # Greedy: eat as many as match, then back off.
  while (1) {
    if (match_here(rest, text, t)) return 1
    if (t > tl) return 0
    if (!atom_match(atom, substr(text, t, 1))) return 0
    t++
  }
}

function match_plus(atom, rest, text, t,   tl) {
  tl = length(text)
  if (t > tl) return 0
  if (!atom_match(atom, substr(text, t, 1))) return 0
  t++
  return match_star(atom, rest, text, t)
}

function match_qmark(atom, rest, text, t) {
  if (t <= length(text) && atom_match(atom, substr(text, t, 1))) {
    if (match_here(rest, text, t + 1)) return 1
  }
  return match_here(rest, text, t)
}

function match_here(pat, text, t,   c1, al, atom, rest, q) {
  if (length(pat) == 0) return 1
  c1 = substr(pat, 1, 1)
  if (c1 == "$" && length(pat) == 1) return (t == length(text) + 1)

  al = atom_len(pat, 1)
  atom = atom_at(pat, 1)
  rest = substr(pat, al + 1)
  q = substr(rest, 1, 1)
  if (q == "*") return match_star(atom, substr(rest, 2), text, t)
  if (q == "+") return match_plus(atom, substr(rest, 2), text, t)
  if (q == "?") return match_qmark(atom, substr(rest, 2), text, t)

  if (t > length(text)) return 0
  if (!atom_match(atom, substr(text, t, 1))) return 0
  return match_here(rest, text, t + 1)
}

function re_match(pat, text,   anchored, p, t) {
  anchored = (substr(pat, 1, 1) == "^")
  p = anchored ? substr(pat, 2) : pat
  if (anchored) return match_here(p, text, 1)
  for (t = 1; t <= length(text) + 1; t++) {
    if (match_here(p, text, t)) return 1
  }
  return 0
}

NF == 0 { next }

{
  pat  = $1
  text = substr($0, length($1) + 2)
  printf "%s\t%s\t%s\n", pat, text, re_match(pat, text) ? "MATCH" : "NOMATCH"
}
