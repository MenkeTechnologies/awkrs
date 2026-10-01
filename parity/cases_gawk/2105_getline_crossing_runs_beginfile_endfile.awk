# gawk runs ENDFILE when plain getline reaches the end of a file and BEGINFILE
# for the file it moves to (and, from BEGIN, for the first file it opens); the
# record loop does not run them again for those files. The input file is read
# twice: ARGV[2] repeats ARGV[1].
BEGIN {
  ARGV[2] = ARGV[1]; ARGC = 3
  getline; print "BEGIN read", $0, NR, FNR
}
BEGINFILE { print "BEGINFILE", ++files }
ENDFILE { print "ENDFILE", files, FNR }
FNR == 2 && files == 1 { while ((getline line) > 0) print "getline", line, NR, FNR }
{ print "rule", $0, NR, FNR }
END { print "END", NR, (getline), NR }
