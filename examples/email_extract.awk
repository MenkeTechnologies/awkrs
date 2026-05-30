# Extract email addresses from free-form text.
# Rule: an email is [A-Za-z0-9._+-]+ @ [A-Za-z0-9.-]+ \. [A-Za-z]{2,}.
# Aggregate one occurrence per (line, position) and emit them in input order,
# then a deduped tally of how many times each address appeared.
#
# Per-line output:
#   "line <n>: <addr1>  <addr2> ..."  (line is the input line number that
#                                       contained matches; lines with no match
#                                       are omitted)
# At end:
#   "TOTALS:"
#   "  <addr>  count=<k>"  for each unique address, sorted lex.

function find_emails(line,   start, m, addr, hits, sep) {
  hits = ""; sep = ""
  start = 1
  while (match(substr(line, start), /[A-Za-z0-9._+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}/)) {
    addr = substr(line, start + RSTART - 1, RLENGTH)
    hits = hits sep addr
    sep = "  "
    tally[addr]++
    start += RSTART + RLENGTH - 1
  }
  return hits
}

{
  h = find_emails($0)
  if (h != "") printf "line %d: %s\n", NR, h
}

END {
  print "TOTALS:"
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (a in tally) printf "  %s  count=%d\n", a, tally[a]
}
