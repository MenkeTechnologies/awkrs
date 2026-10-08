# gawk:2128 — `@load` (and `@include`) end with a statement terminator, so the
# rest of the program may follow a `;` on the same line. awkrs used to drop
# everything after the directive on its line without a diagnostic.
@load "ordchr"; BEGIN { print ord("A"), chr(66) }; END { print "end", NR }
{ print "rec", $0 }
