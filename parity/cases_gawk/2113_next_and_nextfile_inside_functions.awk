# gawk: `next` and `nextfile` inside a function end the rule that called it,
# however deep the call (mawk and one-true-awk reject both at parse time).
function skip_a() { if ($1 == "a") next; return 1 }
function deeper() { skip_a(); return 2 }
function done_file() { nextfile }
{ x = 1 + deeper(); print "kept", $0, x }
NR == 3 { done_file() }
END { print NR }
