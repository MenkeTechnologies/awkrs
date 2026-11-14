//! Reference values for user functions, `delete`/`in`, `printf`, output
//! redirection, regex RS, and the command line.
//!
//! Unless a test says otherwise, every expected value is one that gawk 5.4.1,
//! mawk 1.3.4 and one-true-awk 20200816 all produce. Where the three disagree,
//! the comment names the reference awkrs follows. No reference awk is needed
//! at run time.

mod common;

use common::{run_awkrs_bytes_locale, run_awkrs_stdin, unique_tmp_path};
use std::io::Write;
use std::process::{Command, Stdio};

/// Run awkrs with the full argument vector `args` and `stdin` piped in.
fn awk(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_awkrs"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn awkrs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn ok(program: &str, stdin: &str) -> String {
    let (code, stdout, stderr) = run_awkrs_stdin(program, stdin);
    assert_eq!(code, 0, "program {program:?} failed: {stderr}");
    stdout
}

/// A per-process temp file holding `body`; removed when dropped.
struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new(name: &str, body: &str) -> Self {
        let path = unique_tmp_path(name);
        std::fs::write(&path, body).expect("write fixture");
        Fixture(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("utf-8 temp path")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

// ── user functions ─────────────────────────────────────────────────────────

/// Deep recursion and a result above 2^53 that still prints exactly: 20! is
/// 2432902008176640000, which a double holds without loss.
#[test]
fn recursive_functions() {
    assert_eq!(
        ok(
            "function fact(n){return n<=1?1:n*fact(n-1)}
             function fib(n){return n<2?n:fib(n-1)+fib(n-2)}
             BEGIN{print fact(10); print fact(20); printf \"%d\\n\", fact(20); print fib(20)}",
            ""
        ),
        "3628800\n2432902008176640000\n2432902008176640000\n6765\n"
    );
}

/// Scalars are passed by value; a missing argument is the empty string; a
/// bare `return` yields "" (length 0, not "0").
#[test]
fn scalar_arguments_and_bare_return() {
    assert_eq!(
        ok(
            r#"function f(x){x=5; return}
               function g(a,b){return a "|" b "|"}
               function h(){return}
               BEGIN{y=1; f(y); print y; print g(1) "<" h() ">" length(h())}"#,
            ""
        ),
        "1\n1||<>0\n"
    );
}

/// Arrays are passed by reference: element writes and `delete` inside the
/// callee are visible to the caller.
#[test]
fn array_arguments_are_by_reference() {
    assert_eq!(
        ok(
            r#"function m(a){a[1]="z"; delete a[2]}
               BEGIN{x[1]="a"; x[2]="b"; m(x); print x[1], (2 in x)}"#,
            ""
        ),
        "z 0\n"
    );
}

// ── delete and in ──────────────────────────────────────────────────────────

/// `a[01]` is keyed by the number 1 converted to "1", so the numeric `1.0`
/// finds it and the string "01" does not. `in` never creates an element; a
/// reference such as `a["y"]==""` does.
#[test]
fn subscript_conversion_and_in_does_not_create() {
    assert_eq!(
        ok(
            r#"BEGIN{a[01]=1; print (1.0 in a), ("01" in a), ("1" in a)
               if ("x" in a) ; n=0; for(k in a) n++; print n
               if (a["y"]=="") ; n=0; for(k in a) n++; print n}"#,
            ""
        ),
        "1 0 1\n1\n2\n"
    );
}

/// `delete arr` empties the array and leaves it usable as an array.
#[test]
fn delete_whole_array() {
    assert_eq!(
        ok(
            "BEGIN{a[1];a[2]; delete a; n=0; for(k in a)n++; print n, length(a); a[3]=1; print length(a)}",
            ""
        ),
        "0 0\n1\n"
    );
}

// ── printf ─────────────────────────────────────────────────────────────────

/// `%i` is `%d`: it truncates, and takes `-`, `0`, `+` and precision.
#[test]
fn printf_percent_i_with_flags() {
    assert_eq!(
        ok(
            r#"BEGIN{printf "%-5i|%3i|%i| %05i %+i %.3i\n", 42, -7, 3.9, 12, 5, 7}"#,
            ""
        ),
        "42   | -7|3| 00012 +5 007\n"
    );
}

/// `%c` of a numeric code above 255. In the C locale it is the low byte (all
/// three references print `A`). In a UTF-8 locale gawk prints the character
/// U+0141; mawk and one-true-awk still print `A`. awkrs follows gawk.
#[test]
fn printf_char_code_above_255() {
    let (code, out) = run_awkrs_bytes_locale("C", &[], r#"BEGIN{printf "%c", 321}"#, b"");
    assert_eq!((code, out), (0, b"A".to_vec()));
    let (code, out) = run_awkrs_bytes_locale("en_US.UTF-8", &[], r#"BEGIN{printf "%c", 321}"#, b"");
    assert_eq!((code, out), (0, "\u{141}".as_bytes().to_vec()));
}

/// `%c` of the empty string writes a NUL byte in gawk and mawk; one-true-awk
/// writes nothing. awkrs follows gawk and mawk.
#[test]
fn printf_char_of_empty_string_is_nul() {
    let (code, out) = run_awkrs_bytes_locale("C", &[], r#"BEGIN{printf "[%c]", ""}"#, b"");
    assert_eq!((code, out), (0, b"[\0]".to_vec()));
}

// ── output redirection and close ───────────────────────────────────────────

/// `>` truncates only when the file is first opened; later `print > f` calls
/// append to the open stream. After `close`, `>>` appends and a new `>`
/// truncates again. `close` of a name never opened returns -1.
#[test]
fn redirection_truncate_append_and_close() {
    let f = Fixture::new("awkrs_ref_redir.txt", "old\n");
    let program = format!(
        r#"BEGIN{{f="{}"
           print "a" > f; print "b" > f; close(f)
           while ((getline l < f) > 0) print "1:" l; close(f)
           print "c" >> f; close(f)
           while ((getline l < f) > 0) print "2:" l; close(f)
           print "d" > f; close(f)
           while ((getline l < f) > 0) print "3:" l; close(f)
           print close("never-opened")}}"#,
        f.path()
    );
    assert_eq!(ok(&program, ""), "1:a\n1:b\n2:a\n2:b\n2:c\n3:d\n-1\n");
}

/// Closing an output pipe returns the command's exit status in gawk and mawk
/// (3 here); one-true-awk returns 0. awkrs follows gawk and mawk. Closing an
/// input pipe does the same.
#[test]
fn close_returns_pipe_exit_status() {
    assert_eq!(
        ok(
            r#"BEGIN{c="cat >/dev/null; exit 3"; print "x" | c; print close(c)
               d="cat >/dev/null"; print "x" | d; print close(d)
               e="echo hi; exit 5"; e | getline l; print l, close(e)}"#,
            ""
        ),
        "3\n0\nhi 5\n"
    );
}

// ── RS as a regular expression ─────────────────────────────────────────────

#[test]
fn regex_record_separators() {
    assert_eq!(
        ok(r#"BEGIN{RS="[0-9]+"}{print NR":"$0}"#, "a12b3c"),
        "1:a\n2:b\n3:c\n"
    );
    assert_eq!(
        ok(r#"BEGIN{RS="x|yz"}{print NR":"$0}"#, "axbyzcx"),
        "1:a\n2:b\n3:c\n"
    );
    assert_eq!(
        ok(r#"BEGIN{RS="\n+"}{print NR":"$0}"#, "a\n\n\nb\nc\n"),
        "1:a\n2:b\n3:c\n"
    );
}

/// `^` in a regex RS anchors at the start of the input only, not at the start
/// of each record: the leading `x` makes an empty first record and the later
/// `x`s are ordinary text.
#[test]
fn anchored_rs_matches_only_at_start_of_input() {
    assert_eq!(
        ok(r#"BEGIN{RS="^x"}{print NR":["$0"]"}"#, "xaxbx"),
        "1:[]\n2:[axbx]\n"
    );
}

/// RT holds the text the regex RS matched (empty after the final record).
/// This is gawk's; mawk and one-true-awk have no RT. awkrs follows gawk.
#[test]
fn rt_holds_matched_separator() {
    assert_eq!(
        ok(
            r#"BEGIN{RS="[0-9]+"}{print NR":"$0":"RT":"length(RT)}"#,
            "a12b3c"
        ),
        "1:a:12:2\n2:b:3:1\n3:c::0\n"
    );
}

// ── command line ───────────────────────────────────────────────────────────

/// `-v` values go through escape processing, and a numeric-looking `-v` value
/// is a strnum: it compares numerically but prints as written.
#[test]
fn dash_v_escapes_and_strnum() {
    let (code, stdout, stderr) = awk(
        &[
            "-v",
            r"s=a\tb\\n",
            "-v",
            "n=010",
            "BEGIN{print s; print (n==10), n}",
        ],
        "",
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "a\tb\\n\n1 010\n");
}

/// Several `-f` files form one program: a function defined in a later file
/// is callable from an earlier one.
#[test]
fn multiple_program_files_share_functions() {
    let f1 = Fixture::new("awkrs_ref_f1.awk", "function sq(x){return x*x}\n");
    let f2 = Fixture::new("awkrs_ref_f2.awk", "BEGIN{print sq(7), cube(2)}\n");
    let f3 = Fixture::new("awkrs_ref_f3.awk", "function cube(x){return x*sq(x)}\n");
    let (code, stdout, stderr) = awk(&["-f", f1.path(), "-f", f2.path(), "-f", f3.path()], "");
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "49 8\n");
}

/// ARGV edits made in BEGIN decide which files are read.
#[test]
fn argv_mutation_in_begin() {
    let a = Fixture::new("awkrs_ref_argv_a.txt", "l1\n");
    let b = Fixture::new("awkrs_ref_argv_b.txt", "l2\n");
    let run = |program: &str, files: &[&str]| {
        let mut args = vec![program];
        args.extend_from_slice(files);
        let (code, stdout, stderr) = awk(&args, "stdin\n");
        assert_eq!(code, 0, "{program}: {stderr}");
        stdout
    };
    let both = format!("{} l1\n{} l2\n", a.path(), b.path());
    let only_b = format!("{} l2\n", b.path());

    let append = format!(
        r#"BEGIN{{ARGV[ARGC++]="{}"}} {{print FILENAME, $0}}"#,
        b.path()
    );
    assert_eq!(run(&append, &[a.path()]), both);
    assert_eq!(
        run("BEGIN{ARGC=2} {print FILENAME, $0}", &[a.path(), b.path()]),
        format!("{} l1\n", a.path())
    );
    assert_eq!(
        run(
            "BEGIN{delete ARGV[1]} {print FILENAME, $0}",
            &[a.path(), b.path()]
        ),
        only_b
    );
    assert_eq!(
        run(
            r#"BEGIN{ARGV[1]=""} {print FILENAME, $0}"#,
            &[a.path(), b.path()]
        ),
        only_b
    );
}

/// `var=value` operands take effect when they are reached, between files, and
/// a trailing one is visible in END.
#[test]
fn assignment_operands_apply_in_order() {
    let a = Fixture::new("awkrs_ref_assign_a.txt", "l1\n");
    let b = Fixture::new("awkrs_ref_assign_b.txt", "l2\n");
    let (code, stdout, stderr) = awk(
        &[
            r#"{print v, $0} END{print "end", v}"#,
            "v=1",
            a.path(),
            "v=2",
            b.path(),
            "v=3",
        ],
        "",
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "1 l1\n2 l2\nend 3\n");
}

/// An unreadable `-f` program file and an unreadable input file both exit 2;
/// input already read before the missing file is still processed.
#[test]
fn missing_files_exit_2() {
    let missing = unique_tmp_path("awkrs_ref_missing");
    let missing = missing.to_str().unwrap();
    let (code, stdout, stderr) = awk(&["-f", missing], "");
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.contains("cannot read program file"), "{stderr}");

    let a = Fixture::new("awkrs_ref_missing_in.txt", "l1\n");
    let (code, stdout, stderr) = awk(&["{print}", a.path(), missing], "");
    assert_eq!(code, 2);
    assert_eq!(stdout, "l1\n");
    assert!(stderr.contains("cannot open file"), "{stderr}");
}

/// A bare `exit` in END keeps the status set by an earlier `exit n`.
#[test]
fn bare_exit_in_end_keeps_earlier_status() {
    let (code, stdout, _) = run_awkrs_stdin("{exit 3} END{exit}", "a\n");
    assert_eq!((code, stdout.as_str()), (3, ""));
    let (code, stdout, _) = run_awkrs_stdin(r#"BEGIN{exit 4} END{print "e"; exit}"#, "a\n");
    assert_eq!((code, stdout.as_str()), (4, "e\n"));
}

/// Assigning to ENVIRON does not change the environment of `system()`
/// commands in mawk and one-true-awk; gawk passes the change through. awkrs
/// follows mawk and one-true-awk.
#[test]
fn environ_assignment_does_not_reach_system() {
    let (code, stdout, stderr) = awk(
        &[r#"BEGIN{ENVIRON["AWKRS_REF_T"]="v1"; system("echo [$AWKRS_REF_T]")}"#],
        "",
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "[]\n");
}
