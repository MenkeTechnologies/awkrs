# gawk parity: `IGNORECASE` applies to bare `/regex/` record patterns too,
# not just to `~`/`match`/`gsub`. Previously awkrs's literal-regex fast path
# matched case-sensitively even with `IGNORECASE` set.
BEGIN { IGNORECASE = 1 }
/abc/ { print "matched:", $0 }
