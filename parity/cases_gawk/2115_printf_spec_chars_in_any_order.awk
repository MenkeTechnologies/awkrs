# gawk's format_tree takes flags, width, precision and the meaningless h/l/L
# length modifiers in any order, and abandons a spec on a character it cannot
# accept there (a repeated modifier, a flag after the precision, a second `.`,
# an unknown conversion): the text from `%` through that character is copied
# literally, nothing is converted, and scanning resumes after it.
BEGIN {
    printf "[%l5d] [%5-d] [%h-5d] [%l.3d] [%L5.2f] [%05-d]\n", 1, 2, 3, 4, 5, 6
    printf "[%lld] [%hhd] [%LLd] [%PPd] [%d]\n", 7
    printf "[%5lld] [%-lq] [%5q] [%5.3.2d] [%.3 d] [%d]\n", 8
    printf "[%ll%d] [%l*d] [%l%] [%.-3d]\n", 9, 2, 10, 11
    printf "[%5 0d] [%+ d] [% +d] [%.d] [%-.3d]\n", 12, 13, 14, 0, 15
    s = sprintf("%l5s|%5", "ab"); print s
    printf "%5"; print ""
}
