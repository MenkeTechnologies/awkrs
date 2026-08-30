
# portable:3007 — RS = "" is paragraph mode: records are separated by blank
# lines however many there are, leading blanks are skipped, and a newline
# splits fields in addition to FS.
BEGIN { RS = "" }
{ printf "rec %d: NF=%d $1=[%s] $2=[%s] $3=[%s]\n", NR, NF, $1, $2, $3 }
END { printf "records=%d\n", NR }
