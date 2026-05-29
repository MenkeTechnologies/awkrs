# Minimal INI config parser.
# Recognises:
#   [section]               section header
#   key = value             key/value pair (whitespace around `=` trimmed)
#   # comment / ; comment   skipped
#   blank line              skipped
# Keys outside any [section] go into the "(global)" pseudo-section.
# Output: every (section, key) -> value, sorted by (section asc, key asc),
# then "SECTIONS: <n>  KEYS: <m>".

function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s); return s }

BEGIN { sec = "(global)"; secs[sec] = 1 }

/^[[:space:]]*([#;].*)?$/ { next }

/^[[:space:]]*\[.*\][[:space:]]*$/ {
  line = $0
  sub(/^[[:space:]]*\[/, "", line)
  sub(/\][[:space:]]*$/, "", line)
  sec = trim(line)
  secs[sec] = 1
  next
}

/=/ {
  eq = index($0, "=")
  k = trim(substr($0, 1, eq - 1))
  v = trim(substr($0, eq + 1))
  store[sec, k] = v
  keys_in[sec] = (sec in keys_in) ? keys_in[sec] " " k : k
  nkeys++
  next
}

END {
  PROCINFO["sorted_in"] = "@ind_str_asc"
  nsec = 0
  for (s in secs) {
    nsec++
    if (!(s in keys_in)) continue
    split(keys_in[s], arr, " ")
    n = asort(arr)
    for (i = 1; i <= n; i++) printf "[%s] %s = %s\n", s, arr[i], store[s, arr[i]]
  }
  printf "SECTIONS: %d  KEYS: %d\n", nsec, nkeys + 0
}
