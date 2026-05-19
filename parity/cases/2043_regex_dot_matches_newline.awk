# gawk: in ERE, `.` matches any byte INCLUDING `\n`. awkrs sets
# `dot_matches_new_line(true)` on every regex it builds to match this.
BEGIN {
    s = "ab\ncd"
    print (s ~ /a.*d/)            # 1
    print (s ~ /b.c/)             # 1 (matches across the newline)
    print (s ~ /^a/)              # 1
    print (s ~ /c$/)              # 1
    gsub(/./, "X", s)
    print "[" s "]"               # XXXXX with 5 X's (5 bytes including newline)
}
