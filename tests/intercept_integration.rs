//! End-to-end coverage for the AOP function-call intercept system
//! (awkrs/zshrs-original extension): `intercept("before"|"after"|"around", pattern,
//! code)`, `intercept_proceed()`, `intercept_list()`, `intercept_remove(id)`,
//! `intercept_clear()`, glob patterns, the `INTERCEPT_*` context globals, shared
//! scalar globals between advice and the main program, advice calling other user
//! functions, and re-entrant (nested) interception.

mod common;

use common::run_awkrs_stdin;

// ── before advice ───────────────────────────────────────────────────────────

#[test]
fn before_advice_runs_before_the_function_and_exposes_context() {
    let (c, o, _) = run_awkrs_stdin(
        r#"function greet(n){ return "hi " n }
           BEGIN {
             intercept("before","greet","print \"[before] \" INTERCEPT_NAME \"(\" INTERCEPT_ARGS \")\"")
             print greet("bob")
           }"#,
        "",
    );
    assert_eq!(c, 0);
    assert_eq!(o, "[before] greet(bob)\nhi bob\n");
}

#[test]
fn before_only_does_not_suppress_the_original() {
    // With only before advice, the original must still run once (normal dispatch).
    let (_, o, _) = run_awkrs_stdin(
        r#"function f(){ print "body"; return 0 }
           BEGIN { intercept("before","f","print \"pre\""); f() }"#,
        "",
    );
    assert_eq!(o, "pre\nbody\n");
}

// ── around advice ───────────────────────────────────────────────────────────

#[test]
fn around_advice_with_proceed_runs_the_original_and_returns_its_value() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function add(a,b){ return a+b }
           BEGIN {
             intercept("around","add","print \"[pre]\"; r=intercept_proceed(); print \"[post] r=\" r")
             x = add(2,3)
             print "result=" x
           }"#,
        "",
    );
    // proceed runs the original and captures its return; the join point returns
    // that value transparently to the caller (around wrapping is value-preserving).
    assert_eq!(o, "[pre]\n[post] r=5\nresult=5\n");
}

#[test]
fn around_advice_without_proceed_suppresses_the_original() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function danger(){ print "SHOULD NOT RUN"; return 99 }
           BEGIN { intercept("around","danger","print \"[blocked]\""); x = danger(); print "x=[" x "]" }"#,
        "",
    );
    assert_eq!(o, "[blocked]\nx=[]\n");
}

// ── after advice + timing ───────────────────────────────────────────────────

#[test]
fn after_advice_runs_after_the_original_and_sees_timing() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function work(){ return 7 }
           BEGIN {
             intercept("after","work","print \"[after] ms_set=\" (INTERCEPT_MS+0 >= 0) \" us_set=\" (INTERCEPT_US+0 >= 0)")
             print work()
           }"#,
        "",
    );
    // Original runs, then after advice; INTERCEPT_MS/US are populated numerics.
    assert_eq!(o, "[after] ms_set=1 us_set=1\n7\n");
}

// ── glob patterns ───────────────────────────────────────────────────────────

#[test]
fn glob_pattern_matches_multiple_functions() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function _a(){return 1}
           function _b(){return 2}
           function keep(){return 3}
           BEGIN {
             intercept("before","_*","print \"[hit] \" INTERCEPT_NAME")
             _a(); _b(); keep()
           }"#,
        "",
    );
    assert_eq!(o, "[hit] _a\n[hit] _b\n");
}

#[test]
fn star_pattern_matches_every_function() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function one(){return 1}
           function two(){return 2}
           BEGIN { intercept("before","*","print \"call \" INTERCEPT_NAME"); one(); two() }"#,
        "",
    );
    assert_eq!(o, "call one\ncall two\n");
}

// ── registry management: list / remove / clear ──────────────────────────────

#[test]
fn register_returns_incrementing_ids_and_clear_returns_count() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function f(){return 0}
           BEGIN {
             a = intercept("before","f","print 1")
             b = intercept("after","f","print 2")
             print "ids=" a "," b
             print "cleared=" intercept_clear()
             print "after_clear=" intercept_list()
           }"#,
        "",
    );
    assert_eq!(o, "ids=1,2\ncleared=2\nafter_clear=0\n");
}

#[test]
fn remove_by_id_stops_that_advice_from_firing() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function f(){return 0}
           BEGIN {
             id = intercept("before","f","print \"fires\"")
             f()
             print "removed=" intercept_remove(id)
             print "removed_again=" intercept_remove(id)
             f()
             print "done"
           }"#,
        "",
    );
    // First call fires; after removal (returns 1, then 0), the second call is silent.
    assert_eq!(o, "fires\nremoved=1\nremoved_again=0\ndone\n");
}

// ── shared state: advice <-> main program ───────────────────────────────────

#[test]
fn advice_and_main_share_scalar_globals() {
    // before advice increments a global scalar the main program reads back.
    let (_, o, _) = run_awkrs_stdin(
        r#"function f(){return 0}
           BEGIN { hits=0; intercept("before","f","hits++"); f(); f(); f(); print "hits=" hits }"#,
        "",
    );
    assert_eq!(o, "hits=3\n");
}

#[test]
fn advice_can_call_other_user_functions() {
    let (_, o, _) = run_awkrs_stdin(
        r#"function helper(x){ return x*10 }
           function g(){ return 1 }
           BEGIN { intercept("before","g","print \"helper=\" helper(5)"); g() }"#,
        "",
    );
    assert_eq!(o, "helper=50\n");
}

// ── re-entrancy ─────────────────────────────────────────────────────────────

#[test]
fn nested_interception_fires_for_inner_calls() {
    // outer() is intercepted and its body calls inner(), which is also intercepted.
    let (_, o, _) = run_awkrs_stdin(
        r#"function inner(){ return 0 }
           function outer(){ inner(); return 0 }
           BEGIN {
             intercept("before","inner","print \"in\"")
             intercept("before","outer","print \"out\"")
             outer()
           }"#,
        "",
    );
    assert_eq!(o, "out\nin\n");
}

// ── error handling ──────────────────────────────────────────────────────────

#[test]
fn proceed_outside_around_is_a_runtime_error() {
    let (c, _, e) = run_awkrs_stdin(
        r#"BEGIN { intercept_proceed() }"#,
        "",
    );
    assert_ne!(c, 0);
    assert!(
        e.contains("intercept_proceed"),
        "stderr should name the builtin, got: {e}"
    );
}

#[test]
fn unknown_advice_kind_is_a_runtime_error() {
    let (c, _, e) = run_awkrs_stdin(
        r#"function f(){return 0}
           BEGIN { intercept("sideways","f","print 1") }"#,
        "",
    );
    assert_ne!(c, 0);
    assert!(
        e.contains("before|after|around"),
        "stderr should list valid kinds, got: {e}"
    );
}
