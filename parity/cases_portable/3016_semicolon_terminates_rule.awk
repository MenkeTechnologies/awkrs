# portable:3016 — `;` terminates a rule or function definition the way a
# newline does (POSIX `item_list: item_list item terminator`): pattern-only
# rules, actions and function definitions may each be followed by one `;`.
function twice(x) { return 2 * x };
BEGIN { print "begin", twice(4) };
NR == 1; NR == 3
/two/ { print "two:", $0 }; END { print "end", NR };
