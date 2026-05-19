# Regression: with JIT enabled (the default), patterns combining `~`/`!~` with
# `&&` / `||` dropped matches once the chunk hit the JIT-compile threshold
# (typically the 3rd record). The optimizer now refuses to JIT chunks that mix
# regex match with a short-circuit branch.
/a/ && /b/ { print "AB:", $0 }
/x/ || /y/ { print "XY:", $0 }
