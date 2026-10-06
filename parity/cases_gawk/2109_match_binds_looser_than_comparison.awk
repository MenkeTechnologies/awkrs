# gawk (mawk agrees; one-true-awk rejects the unparenthesized forms): `~` and
# `!~` bind looser than the relational operators, so a comparison on either
# side of a match is evaluated first.
BEGIN {
    x = "ab" ~ "a" == 1; print x          # "ab" ~ ("a" == 1), i.e. ~ "0"
    x = "0" ~ "a" == 1; print x
    x = 2 < 3 ~ 1 < 2; print x            # (2 < 3) ~ (1 < 2)
    x = 1 == 1 ~ 1; print x
    x = "b" !~ "a" < "c"; print x         # "b" !~ ("a" < "c"), i.e. !~ "1"
    print (1 < 2) ~ 1, ("a" ~ "a") == 1
}
