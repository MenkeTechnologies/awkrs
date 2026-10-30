//! Edge-case pins for core POSIX awk semantics: field splitting, strnum
//! coercion, printf conversions, uninitialized values, getline forms, dynamic
//! regexes, NR/FNR across files, and fatal arithmetic errors.
//!
//! Every expected value was taken from gawk 5.x and cross-checked against
//! one-true-awk and mawk where they agree. The tests need no reference awk at
//! run time, so they hold in a headless CI container.

mod common;

use common::{run_awkrs_operands, run_awkrs_stdin, unique_tmp_path};

fn ok(program: &str, stdin: &str) -> String {
    let (code, stdout, stderr) = run_awkrs_stdin(program, stdin);
    assert_eq!(code, 0, "program {program:?} failed: {stderr}");
    stdout
}

/// Write `body` to a per-process temp file and return its path.
fn fixture(name: &str, body: &str) -> std::path::PathBuf {
    let path = unique_tmp_path(name);
    std::fs::write(&path, body).expect("write fixture");
    path
}

// ── field splitting ────────────────────────────────────────────────────────

/// A single-character FS other than space is literal and keeps empty fields;
/// referencing past NF yields the empty string, not an error.
#[test]
fn single_char_fs_keeps_empty_fields() {
    assert_eq!(
        ok(r#"BEGIN{FS=":"}{print NF; print $3 "|" $4}"#, "a:b::c\n"),
        "4\n|c\n"
    );
}

/// Default FS strips leading/trailing blanks and treats runs of spaces and
/// tabs as one separator.
#[test]
fn default_fs_collapses_blank_runs_and_trims() {
    assert_eq!(ok("{print NF, $1}", "  a   b  \t\n"), "2 a\n");
}

/// `.` and `|` as one-character FS are literal characters, not regex
/// metacharacters.
#[test]
fn single_char_fs_metacharacters_are_literal() {
    assert_eq!(ok(r#"BEGIN{FS="."}{print NF, $2}"#, "a.b.c\n"), "3 b\n");
    assert_eq!(ok(r#"BEGIN{FS="|"}{print NF, $2}"#, "a|b|c\n"), "3 b\n");
}

/// `FS="[ ]"` matches exactly one space, so a doubled space produces an empty
/// field — unlike the default single-space FS.
#[test]
fn bracketed_space_fs_is_not_default_splitting() {
    assert_eq!(
        ok(r#"BEGIN{FS="[ ]"}{print NF, "[" $2 "]", $3}"#, "a  b\n"),
        "3 [] b\n"
    );
}

/// A regex FS that matches at the start of the record yields an empty `$1`;
/// leading blanks are only trimmed for the default FS.
#[test]
fn regex_fs_matching_at_start_yields_empty_first_field() {
    assert_eq!(
        ok(r#"BEGIN{FS=" +"}{print NF, "[" $1 "]"}"#, " a b \n"),
        "4 []\n"
    );
    assert_eq!(
        ok(r#"BEGIN{FS="[0-9]+"}{print NF, $3}"#, "a1b22c\n"),
        "3 c\n"
    );
}

/// Empty FS splits into one field per character (gawk/mawk/bwk extension).
#[test]
fn empty_fs_splits_per_character() {
    assert_eq!(ok(r#"BEGIN{FS=""}{print NF, $2}"#, "abc\n"), "3 b\n");
}

/// Changing FS inside a rule only affects the *next* record: the current one
/// was already split with the old value.
#[test]
fn fs_change_applies_to_next_record() {
    assert_eq!(ok(r#"{FS=","; print $1}"#, "a,b\nc,d\n"), "a,b\nc\n");
}

/// `split` with a single-space string uses default splitting (tabs and
/// newlines are blanks too); a `/ /` regex does not.
#[test]
fn split_single_space_string_vs_space_regex() {
    assert_eq!(
        ok(r#"BEGIN{n=split("a\tb\nc  d", a, " "); print n, a[4]}"#, ""),
        "4 d\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{n=split(" a b ", a, / /); print n, "[" a[1] "]"}"#,
            ""
        ),
        "4 []\n"
    );
}

/// A multi-character separator string is a regex, so `+` must be escaped by
/// the regex rules; a one-character `"+"` is literal.
#[test]
fn split_separator_string_regex_rules() {
    assert_eq!(
        ok(
            r#"BEGIN{n=split("1a+b2a+b3", p, "a+b"); print n; n=split("1a+b2", q, "+"); print n, q[1]}"#,
            ""
        ),
        "1\n2 1a\n"
    );
}

/// Paragraph mode: newline always separates fields regardless of FS.
#[test]
fn paragraph_mode_newline_separates_fields_with_custom_fs() {
    assert_eq!(
        ok(
            r#"BEGIN{RS=""; FS=":"} {print NF; for(i=1;i<=NF;i++) printf "[%s]", $i; print ""}"#,
            "a:b\nc\n\n\nd\n"
        ),
        "3\n[a][b][c]\n1\n[d]\n"
    );
}

/// A single-character RS keeps empty records between adjacent separators.
#[test]
fn single_char_rs_keeps_empty_records() {
    assert_eq!(
        ok(r#"BEGIN{RS=";"} {print NR":"$0}"#, "a;b;;c\n"),
        "1:a\n2:b\n3:\n4:c\n\n"
    );
}

// ── record rebuilding and NF ───────────────────────────────────────────────

/// Assigning past NF extends the record with empty fields joined by OFS.
#[test]
fn assigning_beyond_nf_pads_with_empty_fields() {
    assert_eq!(
        ok(r#"{$5="e"; print; print NF}"#, "a b c\n"),
        "a b c  e\n5\n"
    );
}

/// Lowering NF truncates and rebuilds; assigning `$0` resplits.
#[test]
fn nf_assignment_truncates_and_resplit_on_dollar_zero() {
    assert_eq!(
        ok(r#"{NF=2; print; $0="x y z"; print $2, NF}"#, "a b c\n"),
        "a b\ny 3\n"
    );
    assert_eq!(ok(r#"{NF=0; print "[" $0 "]", NF}"#, "a b\n"), "[] 0\n");
}

/// The record is rebuilt with the OFS in force *at rebuild time*: changing OFS
/// alone does not alter an already-rebuilt `$0` until a field is assigned.
#[test]
fn record_rebuild_uses_ofs_at_rebuild_time() {
    assert_eq!(
        ok(
            r#"BEGIN{ $0="a b c"; NF=5; print; OFS=":"; print; $1=$1; print }"#,
            ""
        ),
        "a b c  \na b c  \na:b:c::\n"
    );
}

/// `$1=$1` collapses the original whitespace runs into OFS.
#[test]
fn self_assignment_of_field_normalises_whitespace() {
    assert_eq!(
        ok(
            r#"BEGIN{ $0 = "  lead  trail  "; print NF, "[" $1 "]"; $1 = $1; print "[" $0 "]" }"#,
            ""
        ),
        "2 [lead]\n[lead trail]\n"
    );
}

/// `$0` and NF from the last record survive into END.
#[test]
fn last_record_is_visible_in_end() {
    assert_eq!(ok("END{print $0, NF, $2}", "x y\na b c\n"), "a b c 3 b\n");
}

/// Negative field index and negative NF are fatal (exit 2) in every reference.
#[test]
fn negative_field_index_and_negative_nf_are_fatal() {
    let (code, stdout, stderr) = run_awkrs_stdin("{print $(-1)}", "a b\n");
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.contains("-1"), "stderr: {stderr}");

    let (code, _, stderr) = run_awkrs_stdin("{NF=-1}", "a b\n");
    assert_eq!(code, 2);
    assert!(stderr.contains("NF"), "stderr: {stderr}");
}

// ── number / string coercion ───────────────────────────────────────────────

/// Uninitialized variables are both 0 and "" at once.
#[test]
fn uninitialized_scalar_is_zero_and_empty() {
    assert_eq!(
        ok(
            r#"BEGIN{print x+0, "[" x "]", length(x), (x==0), (x=="")}"#,
            ""
        ),
        "0 [] 0 1 1\n"
    );
}

/// A bare `return` gives the caller the uninitialized value, and an untyped
/// parameter passed into a function becomes the caller's array once used as one.
#[test]
fn untyped_parameter_becomes_caller_array() {
    assert_eq!(
        ok(
            r#"function f(arr) { arr["k"]=1 } BEGIN{ f(z); f(z); print length(z), z["k"] }"#,
            ""
        ),
        "1 1\n"
    );
    assert_eq!(
        ok(
            r#"function f(s) { s = "changed" } BEGIN { v = "orig"; f(v); print v }"#,
            ""
        ),
        "orig\n"
    );
}

/// Field values that look numeric compare numerically (strnum); string
/// constants never do.
#[test]
fn strnum_fields_compare_numerically_constants_do_not() {
    assert_eq!(ok(r#"{print ($1<$2), ("10"<"9")}"#, "10 9\n"), "0 1\n");
    assert_eq!(ok(r#"{print ($1==3), ($1=="3")}"#, "3.0\n"), "1 0\n");
    assert_eq!(ok(r#"{print ($1 == 100), ($1 < 2)}"#, "1e2\n"), "1 0\n");
    // Leading/trailing blanks still leave `$0` a strnum.
    assert_eq!(ok(r#"{print ($0 == 10), ($0 < 9)}"#, " 10 \n"), "1 0\n");
}

/// A non-numeric field compares as a string against a number.
#[test]
fn non_numeric_field_compares_as_string() {
    assert_eq!(ok(r#"{print ($1 < 2), ($1 > 2)}"#, "abc\n"), "0 1\n");
}

/// String-to-number conversion takes the longest numeric prefix, accepts
/// exponents and leading blanks/sign, and ignores hex in input data.
#[test]
fn string_to_number_uses_leading_numeric_prefix() {
    assert_eq!(ok("{print $1+0, $1*2}", " +3.0abc\n"), "3 6\n");
    assert_eq!(ok("{print $1+0, $2+0}", "1e3 0x1A\n"), "1000 0\n");
    assert_eq!(
        ok(r#"BEGIN{print -"3", +"4x", -"", int("4.9x"), int("")}"#, ""),
        "-3 4 0 4 0\n"
    );
}

/// Number-to-string uses CONVFMT for concatenation and OFMT for print, but
/// integral values always print as integers.
#[test]
fn convfmt_and_ofmt_are_distinct() {
    assert_eq!(
        ok(
            r#"BEGIN{print 1/3; OFMT="%.2f"; print 1/3; x=1/3; print x ""; CONVFMT="%.3f"; print x ""; print 17 ""}"#,
            ""
        ),
        "0.333333\n0.33\n0.333333\n0.333\n17\n"
    );
}

/// Array subscripts go through CONVFMT, except integral values.
#[test]
fn numeric_subscripts_use_convfmt() {
    assert_eq!(
        ok(
            r#"BEGIN{ x = 12.7; a[x]=1; for(k in a) print k; print (12.7 in a) }"#,
            ""
        ),
        "12.7\n1\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{x["1"] = "a"; print x[1], x[1.0], "[" x["01"] "]"}"#,
            ""
        ),
        "a a []\n"
    );
}

/// Large integers print exactly up to 2^53 and in %d form beyond `%.6g`
/// range; non-integral values use `%.6g`.
#[test]
fn print_of_large_and_fractional_numbers() {
    assert_eq!(
        ok(
            r#"BEGIN{ print 0.1+0.2, 1e6, 1e16, 123456789012, 0.000001 }"#,
            ""
        ),
        "0.3 1000000 10000000000000000 123456789012 1e-06\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{print 100000000000000000000; print 2^53+1; print -0}"#,
            ""
        ),
        "100000000000000000000\n9007199254740992\n0\n"
    );
}

/// Exponent is right-associative and binds tighter than unary minus; `%`
/// follows C fmod (sign of the dividend).
#[test]
fn arithmetic_precedence_and_modulo_sign() {
    assert_eq!(
        ok(r#"BEGIN{print 5%3, -5%3, 5%-3, 2^3^2, -2^2, 10 % 3.5}"#, ""),
        "2 -2 2 512 -4 3\n"
    );
}

/// Binary minus binds tighter than concatenation: `1 " " -1` is
/// `1 (" " - 1)`, i.e. "1" concatenated with -1, while `1 -1` subtracts.
#[test]
fn concatenation_versus_binary_minus() {
    assert_eq!(ok(r#"BEGIN{print 1 " " -1; print 1 -1}"#, ""), "1-1\n0\n");
}

/// Truthiness: numeric zero and the empty string are false; the string "0"
/// constant is true, but a strnum field "0" is false.
#[test]
fn truthiness_of_string_zero_vs_strnum_zero() {
    assert_eq!(ok(r#"BEGIN{print !0, !"", !"a", !"0"}"#, ""), "1 1 0 0\n");
    assert_eq!(ok(r#"{print !$1, ($1 ? "t" : "f")}"#, "0\n"), "1 f\n");
}

// ── printf ─────────────────────────────────────────────────────────────────

#[test]
fn printf_width_precision_and_flags() {
    assert_eq!(
        ok(
            r#"BEGIN{printf "%5.2f|%-5d|%05d|%x|%o|%e|%c|%c\n", 3.14159, 42, 42, 255, 8, 1234.5, 65, "hello"}"#,
            ""
        ),
        " 3.14|42   |00042|ff|10|1.234500e+03|A|h\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{printf "[%8.3f] [%-8.3f] [%+8.3f] [%08.3f]\n", -3.14159, 3.14159, 3.14159, -3.14159}"#,
            ""
        ),
        "[  -3.142] [3.142   ] [  +3.142] [-003.142]\n"
    );
    assert_eq!(
        ok(r#"BEGIN{printf "%#o %#x %+d % d\n", 8, 255, 5, 5}"#, ""),
        "010 0xff +5  5\n"
    );
}

/// `*` takes width from the argument list; a negative `*` width left-aligns.
#[test]
fn printf_star_width() {
    assert_eq!(
        ok(
            r#"BEGIN{printf "%*d|%-*s|%*d|\n", 5, 3, 4, "ab", -4, 7}"#,
            ""
        ),
        "    3|ab  |7   |\n"
    );
}

/// Precision on `%s` truncates; `%d` truncates toward zero and reads the
/// numeric prefix of a string.
#[test]
fn printf_string_precision_and_integer_truncation() {
    assert_eq!(
        ok(
            r#"BEGIN{printf "%.3s|%10.3s|%5.1s|\n", "abcdef", "abcdef", "xyz"}"#,
            ""
        ),
        "abc|       abc|    x|\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{printf "%d %d %i %d\n", -3.9, "12abc", 7.9, "1e2"}"#,
            ""
        ),
        "-3 12 7 100\n"
    );
}

/// `%.0f` rounds half to even (C printf); `%g` switches to exponent form at
/// 1e6 and below 1e-4.
#[test]
fn printf_rounding_and_g_thresholds() {
    assert_eq!(
        ok(r#"BEGIN{printf "%.0f|%.0f|%.0f\n", 2.5, 3.5, -0.5}"#, ""),
        "2|4|-0\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{printf "%g %g %g %g %G\n", 100000, 1000000, 0.0001, 0.00001, 1e-10}"#,
            ""
        ),
        "100000 1e+06 0.0001 1e-05 1E-10\n"
    );
}

/// `%%` emits a literal percent; `%c` of a string takes its first character
/// and of a number the character with that code. (`%c` of "" is left out:
/// gawk and mawk emit NUL, one-true-awk emits nothing.)
#[test]
fn printf_percent_and_char_edge_cases() {
    assert_eq!(
        ok(r#"BEGIN{printf "%%|%s\n", sprintf("%d%%", 50)}"#, ""),
        "%|50%\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{printf "%c%c|%5c|%-5c|\n", 0+"65", "ABC", "a", 66}"#,
            ""
        ),
        "AA|    a|B    |\n"
    );
}

/// Too few arguments for the format is a fatal error (exit 2) with nothing
/// printed — all three references refuse.
#[test]
fn printf_with_missing_arguments_is_fatal() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN{printf "%s %d\n", "a"}"#, "");
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.contains("argument"), "stderr: {stderr}");
}

// ── string builtins ────────────────────────────────────────────────────────

/// substr rounds its start/length and clips to the string. Values follow
/// gawk and one-true-awk, which agree here (mawk differs on a negative start).
#[test]
fn substr_rounding_and_clipping() {
    assert_eq!(
        ok(
            r#"BEGIN{print substr("hello",0,2), substr("hello",-1,3), substr("hello",2), substr("hello",1.5,2.3)}"#,
            ""
        ),
        "he hel ello he\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{print "[" substr("hello", 2, -1) "]", "[" substr("hello", 10) "]", substr(12345, 2, 3)}"#,
            ""
        ),
        "[] [] 234\n"
    );
}

/// gsub with a regex that can match empty inserts at every position that is
/// not adjacent to a previous match.
#[test]
fn gsub_with_empty_matching_regex() {
    assert_eq!(
        ok(r#"BEGIN{s="aaa"; n=gsub(/a*/,"-",s); print n, s}"#, ""),
        "1 -\n"
    );
    assert_eq!(
        ok(r#"BEGIN{s="abc"; n=gsub(/x*/,"-",s); print n, s}"#, ""),
        "4 -a-b-c-\n"
    );
    assert_eq!(
        ok(
            r#"BEGIN{s="abc"; gsub(/$/, "!", s); print s; t="aaa"; gsub(/^a/, "b", t); print t}"#,
            ""
        ),
        "abc!\nbaa\n"
    );
}

/// `&` in the replacement is the matched text; `\\&` is a literal ampersand.
#[test]
fn gsub_ampersand_and_escaped_ampersand() {
    assert_eq!(
        ok(
            r#"BEGIN{s="hello"; gsub(/l/,"[&]",s); print s; t="hello"; gsub(/l/,"\\&",t); print t}"#,
            ""
        ),
        "he[l][l]o\nhe&&o\n"
    );
}

/// A string first argument to sub/gsub is a dynamic regex, so `"."` matches
/// any character.
#[test]
fn sub_string_pattern_is_dynamic_regex() {
    assert_eq!(
        ok(
            r#"BEGIN{s="foo.bar"; sub(".", "X", s); print s; t="a.b.c"; print gsub(".", "-", t), t}"#,
            ""
        ),
        "Xoo.bar\n5 -----\n"
    );
}

/// gsub on `$0` resplits the record; sub on a field rebuilds `$0`.
#[test]
fn gsub_on_record_and_sub_on_field() {
    assert_eq!(ok(r#"{gsub(/b/, " "); print NF, $2}"#, "abc\n"), "2 c\n");
    assert_eq!(
        ok(r#"{sub(/b/, "X", $2); print; print NF}"#, "a b c\n"),
        "a X c\n3\n"
    );
}

/// match sets RSTART/RLENGTH, including the zero-length and no-match cases.
#[test]
fn match_sets_rstart_and_rlength() {
    assert_eq!(
        ok(
            r#"BEGIN{print match("xxabcx", /ab+c/), RSTART, RLENGTH; print match("x",/z/), RSTART, RLENGTH; print match("", /^/), RSTART, RLENGTH}"#,
            ""
        ),
        "3 3 3\n0 0 -1\n1 1 0\n"
    );
}

#[test]
fn index_and_length_edge_cases() {
    assert_eq!(
        ok(
            r#"BEGIN{print index("abc",""), index("","a"), index("abc","abcd"), index(123456, 34), length(0.10), length(12345)}"#,
            ""
        ),
        "1 0 0 3 3 5\n"
    );
    assert_eq!(ok("{print length(), length}", "hello\n"), "5 5\n");
}

// ── dynamic regex ──────────────────────────────────────────────────────────

/// A string used with `~` is compiled as a regex at run time, with string
/// escapes processed first (so `"a\\.c"` is the regex `a\.c`).
#[test]
fn dynamic_regex_from_string() {
    assert_eq!(
        ok(
            r#"BEGIN{r="a.c"; print ("abc" ~ r), ("a.c" ~ r), ("abd" ~ r); r="a\\.c"; print ("abc" ~ r), ("a.c" ~ r)}"#,
            ""
        ),
        "1 1 0\n0 1\n"
    );
    // The record itself as a regex on the right-hand side.
    assert_eq!(
        ok(r#"{print ("ax" ~ $0), ("ax" ~ ("^" $0))}"#, "x\n"),
        "1 0\n"
    );
}

/// An invalid dynamic regex is a fatal run-time error.
#[test]
fn invalid_dynamic_regex_is_fatal() {
    let (code, stdout, stderr) = run_awkrs_stdin(r#"BEGIN{r = "["; print ("[" ~ r)}"#, "");
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.contains("regexp"), "stderr: {stderr}");
}

// ── getline ────────────────────────────────────────────────────────────────

/// Plain `getline` replaces `$0`, NF, NR and FNR; `getline var` sets only var,
/// NR and FNR, leaving `$0` and NF alone.
#[test]
fn getline_main_input_forms_update_counters() {
    assert_eq!(
        ok(
            r#"NR==1{getline v; print v, NF, $0, NR, FNR}"#,
            "a b c\nd e\n"
        ),
        "d e 3 a b c 2 2\n"
    );
    assert_eq!(
        ok(r#"{getline; print "got", $0, NF, NR}"#, "a\nb c\n"),
        "got b c 2 2\n"
    );
}

/// `cmd | getline` sets `$0` and NF; `cmd | getline var` sets only var.
/// Neither touches NR or FNR in gawk, mawk or one-true-awk.
#[test]
fn command_getline_forms_leave_counters_alone() {
    assert_eq!(
        ok(
            r#"{"echo x y" | getline; print NF, $2, NR, FNR}"#,
            "a b c\n"
        ),
        "2 y 1 1\n"
    );
    assert_eq!(
        ok(
            r#"{"echo x y" | getline w; print NF, w, NR, FNR, $0}"#,
            "a b c\n"
        ),
        "3 x y 1 1 a b c\n"
    );
}

/// `getline var < file` touches neither NR nor FNR; after close the file is
/// reread from the start. A missing file returns -1.
#[test]
fn file_getline_does_not_touch_nr_and_close_rewinds() {
    let path = fixture("awkrs_edges_getline.txt", "a\nb\n");
    let p = path.display();
    let program = format!(
        r#"BEGIN{{while((getline l < "{p}")>0) print "l", l, NR; close("{p}"); getline < "{p}"; print $0, NR; print (getline x < "/nonexistent/awkrs_zz")}}"#
    );
    let out = ok(&program, "");
    std::fs::remove_file(&path).ok();
    assert_eq!(out, "l a 0\nl b 0\na 0\n-1\n");
}

/// Reading a command to exhaustion, then closing it, lets it be rerun.
#[test]
fn command_getline_loop_and_close_reruns() {
    assert_eq!(
        ok(
            r#"BEGIN{c="printf \"1\\n2\\n\""; while ((c | getline x) > 0) s+=x; close(c); while ((c | getline x) > 0) s+=x; print s}"#,
            ""
        ),
        "6\n"
    );
}

/// `getline` in BEGIN consumes the first record of the main input.
#[test]
fn getline_in_begin_reads_first_record() {
    assert_eq!(
        ok(
            r#"BEGIN{getline; print "B:" $0, NR} {print "M:" $0}"#,
            "one\ntwo\n"
        ),
        "B:one 1\nM:two\n"
    );
}

// ── NR / FNR across files and ARGV ─────────────────────────────────────────

/// FNR resets per file, NR does not; FILENAME tracks the current operand.
#[test]
fn nr_and_fnr_across_two_files() {
    let f1 = fixture("awkrs_edges_nr1.txt", "a\nb\n");
    let f2 = fixture("awkrs_edges_nr2.txt", "c\nd\ne\n");
    let (code, stdout, stderr) = run_awkrs_operands(
        r#"FNR==1{print "file", (NR>1 ? 2 : 1)} {print FNR, NR, $0} END{print NR, FNR}"#,
        [f1.to_str().unwrap(), f2.to_str().unwrap()],
        "",
    );
    std::fs::remove_file(&f1).ok();
    std::fs::remove_file(&f2).ok();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        "file 1\n1 1 a\n2 2 b\nfile 2\n1 3 c\n2 4 d\n3 5 e\n5 3\n"
    );
}

/// A `var=value` operand between files takes effect before the next file is
/// read; an empty ARGV slot set in BEGIN is skipped.
#[test]
fn argv_assignment_operand_and_empty_slot() {
    let f1 = fixture("awkrs_edges_argv1.txt", "a\nb\n");
    let f2 = fixture("awkrs_edges_argv2.txt", "c\n");
    let (code, stdout, stderr) = run_awkrs_operands(
        "{print v+0, $0}",
        [f1.to_str().unwrap(), "v=7", f2.to_str().unwrap()],
        "",
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "0 a\n0 b\n7 c\n");

    let program = format!(
        r#"BEGIN{{ARGV[1]="{}"; ARGV[2]=""; ARGV[3]="{}"; ARGC=4}} END{{print NR}}"#,
        f1.display(),
        f2.display()
    );
    let out = ok(&program, "ignored\n");
    std::fs::remove_file(&f1).ok();
    std::fs::remove_file(&f2).ok();
    assert_eq!(out, "3\n");
}

/// Plain `getline` crosses file boundaries: FNR resets when it pulls the first
/// record of the next file.
#[test]
fn getline_crosses_file_boundary() {
    let f1 = fixture("awkrs_edges_cross1.txt", "a\n");
    let f2 = fixture("awkrs_edges_cross2.txt", "b\nc\n");
    let (code, stdout, stderr) = run_awkrs_operands(
        r#"NR==1{getline; print $0, NR, FNR, (FILENAME ~ /cross2/)}"#,
        [f1.to_str().unwrap(), f2.to_str().unwrap()],
        "",
    );
    std::fs::remove_file(&f1).ok();
    std::fs::remove_file(&f2).ok();
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "b 2 1 1\n");
}

// ── fatal arithmetic ───────────────────────────────────────────────────────

/// Division and modulo by zero are fatal (exit 2) in gawk and one-true-awk,
/// including the compound-assignment and field-operand paths; output already
/// produced is kept.
#[test]
fn division_by_zero_is_fatal_on_every_path() {
    for program in [
        "BEGIN{print 1/0}",
        "BEGIN{print 5%0}",
        "BEGIN{x=1; x/=0; print x}",
        "BEGIN{x=1; x%=0; print x}",
        "{print $1/0}",
    ] {
        let (code, stdout, stderr) = run_awkrs_stdin(program, "4\n");
        assert_eq!(code, 2, "{program}");
        assert_eq!(stdout, "", "{program}");
        assert!(stderr.contains("division by zero"), "{program}: {stderr}");
    }
    let (code, stdout, _) = run_awkrs_stdin(r#"{print "before"; print $1/$2}"#, "1 0\n");
    assert_eq!(code, 2);
    assert_eq!(stdout, "before\n");
}

// ── control flow ───────────────────────────────────────────────────────────

/// `exit` in a main rule still runs END; `exit N` in BEGIN skips input but
/// still runs END and keeps the status.
#[test]
fn exit_runs_end_and_keeps_status() {
    assert_eq!(ok(r#"{exit} END{print "end", NR}"#, "a\nb\n"), "end 1\n");
    let (code, stdout, _) =
        run_awkrs_stdin(r#"BEGIN{exit 3} {print "main"} END{print "end"}"#, "a\n");
    assert_eq!(code, 3);
    assert_eq!(stdout, "end\n");
}

/// Increment/decrement evaluation order matches the references.
#[test]
fn increment_decrement_values() {
    assert_eq!(
        ok(
            r#"BEGIN{i=5; print i++ + ++i, i; print i--, i; a = "A"; b = a++; print a, b}"#,
            ""
        ),
        "12 7\n7 6\n1 0\n"
    );
}

/// SUBSEP joins multi-dimensional subscripts and `(i,j) in a` tests them.
#[test]
fn multidimensional_subscripts_use_subsep() {
    assert_eq!(
        ok(
            r#"BEGIN{print ((1,2) in a); a[1,2]=3; for (k in a) {split(k,p,SUBSEP); print p[1], p[2]}; print ((1,2) in a); SUBSEP=":"; b["x","y"]; for (k in b) print k}"#,
            ""
        ),
        "0\n1 2\n1\nx:y\n"
    );
}
