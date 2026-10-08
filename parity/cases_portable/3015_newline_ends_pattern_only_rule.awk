# portable:3015 — a newline ends a pattern-only rule: `NR == 1` <NL> `{ ... }`
# is two rules (print the matching record; run the action on every record),
# for an expression pattern, a regex pattern and a range pattern alike.
NR == 1
{ print "a:" $0 }
/two/
{ print "b:" $0 }
/one/, /two/
{ print "c:" $0 }
/thr/
NR == 3
