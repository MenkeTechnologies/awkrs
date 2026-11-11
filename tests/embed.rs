//! `awkrs::run_program`: the in-process embedding entry point.

use awkrs::run_program;

#[test]
fn begin_print_is_captured() {
    assert_eq!(
        run_program(r#"BEGIN { print "hello" }"#, "").unwrap(),
        "hello\n"
    );
}

#[test]
fn input_is_the_primary_stream_and_honors_rs_from_begin() {
    let out = run_program(
        r#"BEGIN { RS = ";" } { print NR ": " toupper($2) }"#,
        "a b;c d;e f",
    )
    .unwrap();
    assert_eq!(out, "1: B\n2: D\n3: F\n");
}

#[test]
fn getline_in_begin_reads_input() {
    let out = run_program(
        r#"BEGIN { while ((getline l) > 0) n++; print n }"#,
        "x\ny\nz\n",
    )
    .unwrap();
    assert_eq!(out, "3\n");
}

#[test]
fn errors_and_nonzero_exit_return_err() {
    assert!(run_program("BEGIN { print ( }", "").is_err());
    assert_eq!(run_program("BEGIN { exit 3 }", "").unwrap_err(), "exit 3");
    assert_eq!(
        run_program(r#"{ print; exit } END { print "end" }"#, "a\nb\n").unwrap(),
        "a\nend\n"
    );
}

#[test]
fn large_output_stays_captured() {
    // Large output plus `fflush()`, which writes to fd 1 outside capture mode.
    let out = run_program(
        r#"BEGIN { for (i = 0; i < 200000; i++) print i; fflush() }"#,
        "",
    )
    .unwrap();
    assert_eq!(out.lines().count(), 200_000);
    assert!(out.ends_with("199999\n"));
}

#[test]
fn concurrent_calls_keep_their_own_output() {
    let handles: Vec<_> = (0..8)
        .map(|t| {
            std::thread::spawn(move || {
                let input: String = (0..2000).map(|i| format!("{t} {i}\n")).collect();
                (t, run_program("{ print $1 }", &input).unwrap())
            })
        })
        .collect();
    for h in handles {
        let (t, out) = h.join().unwrap();
        assert!(
            out.lines().all(|l| l == t.to_string()),
            "thread {t} saw foreign output"
        );
        assert_eq!(out.lines().count(), 2000);
    }
}
