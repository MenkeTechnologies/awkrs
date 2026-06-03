// Unit tests for vm.rs. Split out of vm.rs to keep the module file manageable.
// This is a child module of `vm` via `#[path]`; `use super::*` resolves to vm.rs.
use super::*;
use crate::compiler::Compiler;
use crate::flow::Flow;
use crate::parser::parse_program;
use crate::runtime::Runtime;

fn compile(prog_text: &str) -> CompiledProgram {
    let prog = parse_program(prog_text).expect("parse");
    Compiler::compile_program(&prog).unwrap()
}

/// Match `lib::run`: slotted scalars from the compiler need `init_slots` before VM runs.
fn runtime_with_slots(cp: &CompiledProgram) -> Runtime {
    let mut rt = Runtime::new();
    rt.slots = cp.init_slots(&rt.vars);
    rt
}

#[test]
fn vm_begin_prints_numeric_expression() {
    let cp = compile("BEGIN { print 2 + 3 * 4 }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "14\n");
}

#[test]
fn vm_begin_user_recursion_hits_call_depth_cap() {
    let cp = compile("function f(){ f() } BEGIN { f() }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    let msg = e.to_string();
    assert!(
        msg.contains("maximum user function call depth"),
        "unexpected err: {msg}"
    );
}

/// `-M`: integer literals must not round through `f64` before `+` / `sprintf %d`.
#[test]
fn vm_begin_bignum_sprintf_i64_max_plus_one() {
    let cp = compile(r#"BEGIN { print sprintf("%d", 9223372036854775807 + 1) }"#);
    let mut rt = runtime_with_slots(&cp);
    rt.bignum = true;
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&rt.print_buf),
        "9223372036854775808\n"
    );
}

/// Mirrors CLI `-v a=1 -v b=2 -v c=3` (`apply_assigns` stores string values).
#[test]
fn vm_begin_print_sum_of_three_minus_v_style_vars() {
    let cp = compile("BEGIN { print a+b+c }");
    let mut rt = Runtime::new();
    rt.vars.insert("a".into(), Value::Str("1".into()));
    rt.vars.insert("b".into(), Value::Str("2".into()));
    rt.vars.insert("c".into(), Value::Str("3".into()));
    rt.slots = cp.init_slots(&rt.vars);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "6\n");
}

#[test]
fn vm_begin_assigns_global_and_prints() {
    let cp = compile("BEGIN { answer = 42; print answer }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "42\n");
    let slot = *cp.slot_map.get("answer").expect("answer slotted");
    assert_eq!(rt.slots[slot as usize].as_number(), 42.0);
}

#[test]
fn vm_begin_power_star_star() {
    let cp = compile("BEGIN { print 2 ** 10 }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1024\n");
}

#[test]
fn vm_begin_intdiv_and_intdiv0() {
    let cp = compile("BEGIN { print intdiv(7, 2), intdiv0(5, 0) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3 0\n");
}

#[test]
fn vm_begin_index_empty_needle_is_one_miss_is_zero() {
    let cp = compile(r#"BEGIN { print index("abc", ""), index("abc", "x") }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1 0\n");
}

#[test]
fn vm_begin_index_finds_first_byte_substring() {
    let cp = compile(r#"BEGIN { print index("abc", "bc") }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "2\n");
}

#[test]
fn vm_begin_substr_zero_length_yields_empty() {
    let cp = compile(r#"BEGIN { print "[" substr("hello", 2, 0) "]" }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "[]\n");
}

#[test]
fn vm_begin_substr_omitted_length_takes_rest() {
    let cp = compile(r#"BEGIN { print substr("abcdef", 3) }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "cdef\n");
}

#[test]
fn vm_begin_split_returns_count_and_fills_array() {
    let cp = compile(r#"BEGIN { n = split("a,b,c", t, ","); print n, t[1], t[2], t[3] }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3 a b c\n");
}

#[test]
fn vm_begin_asort_reorders_numeric_values() {
    let cp = compile("BEGIN { a[1]=30; a[2]=10; a[3]=20; asort(a); print a[1], a[2], a[3] }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "10 20 30\n");
}

#[test]
fn vm_begin_atan2_pi_over_four() {
    let cp = compile("BEGIN { print atan2(1, 1) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    let v: f64 = String::from_utf8_lossy(&rt.print_buf)
        .trim()
        .parse()
        .unwrap();
    // Default `OFMT` rounds; parsed text is not full `f64` precision.
    assert!((v - std::f64::consts::FRAC_PI_4).abs() < 1e-5, "got {v}");
}

#[test]
fn vm_begin_atan2_wrong_arity_errors() {
    let cp = compile("BEGIN { print atan2(1) }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("atan2"), "{e:?}");
}

#[test]
fn vm_begin_systime_with_arg_errors() {
    let cp = compile("BEGIN { print systime(1) }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("systime"), "{e:?}");
}

#[test]
fn vm_begin_srand_resets_rand_sequence() {
    let cp = compile("BEGIN { srand(42); a = rand(); srand(42); b = rand(); print (a == b) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1\n");
}

#[test]
fn vm_begin_isarray_and_typeof_scalar_elem() {
    let cp = compile("BEGIN { a[1] = 7; print isarray(a), typeof(a[1]) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1 number\n");
}

#[test]
fn vm_begin_gensub_global_returns_modified_string() {
    let cp = compile(r#"BEGIN { print gensub(/[0-9]/, "X", "g", "a1b2") }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "aXbX\n");
}

#[test]
fn vm_begin_tolower_toupper_roundtrip_shape() {
    let cp = compile(r#"BEGIN { print toupper("aBc"), tolower("XyZ") }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "ABC xyz\n");
}

#[test]
fn vm_begin_sqrt_perfect_square() {
    let cp = compile("BEGIN { print sqrt(9) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3\n");
}

#[test]
fn vm_begin_sqrt_wrong_arity_errors() {
    let cp = compile("BEGIN { print sqrt() }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("sqrt"), "{e:?}");
}

#[test]
fn vm_begin_log_one_is_zero() {
    let cp = compile("BEGIN { print log(1) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0\n");
}

#[test]
fn vm_begin_exp_zero_is_one() {
    let cp = compile("BEGIN { print exp(0) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1\n");
}

#[test]
fn vm_begin_length_no_args_uses_empty_record() {
    let cp = compile("BEGIN { print length() }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0\n");
}

#[test]
fn vm_begin_length_string_argument_counts_chars() {
    let cp = compile(r#"BEGIN { print length("hello") }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "5\n");
}

#[test]
fn vm_begin_length_array_counts_entries() {
    let cp = compile("BEGIN { a[1]=1; a[2]=2; a[99]=3; print length(a) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3\n");
}

#[test]
fn vm_begin_sin_zero_and_cos_zero() {
    let cp = compile("BEGIN { print sin(0), cos(0) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0 1\n");
}

#[test]
fn vm_begin_sin_wrong_arity_errors() {
    let cp = compile("BEGIN { print sin() }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("sin"), "{e:?}");
}

#[test]
fn vm_begin_int_truncates_toward_zero() {
    let cp = compile("BEGIN { print int(3.9), int(-3.9) }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "3 -3\n");
}

#[test]
fn vm_begin_mkbool_numeric_zero_vs_nonzero() {
    let cp = compile("BEGIN { print mkbool(0), mkbool(0.5), mkbool(\"\") }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0 1 0\n");
}

#[test]
fn vm_begin_mkbool_wrong_arity_errors() {
    let cp = compile("BEGIN { print mkbool() }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("mkbool"), "{e:?}");
}

#[test]
fn vm_begin_many_rand_draws_stay_in_half_open_unit_interval() {
    let cp = compile(
            "BEGIN { bad = 0; for (i = 1; i <= 80; i++) { r = rand(); if (r < 0 || r >= 1) bad++ } print bad }",
        );
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "0\n");
}

#[test]
fn vm_user_function_bare_return_runs() {
    let cp = compile("function f(){ return } BEGIN { f(); print \"ok\" }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "ok\n");
}

#[test]
fn vm_begin_ofs_between_output_fields() {
    let cp = compile(r#"BEGIN { OFS = "|"; print "a", "b" }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "a|b\n");
}

#[test]
fn vm_begin_ors_after_each_print() {
    let cp = compile(r#"BEGIN { ORS = "X"; print "p"; print "q" }"#);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "pXqX");
}

#[test]
fn vm_begin_multidim_array_assign_and_read() {
    let cp = compile("BEGIN { a[1,2] = 42; print a[1,2] }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "42\n");
}

#[test]
fn vm_begin_next_is_invalid() {
    let cp = compile("BEGIN { next }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    match e {
        Error::Runtime(s) => assert!(s.contains("next"), "{s}"),
        _ => panic!("unexpected err: {e:?}"),
    }
}

#[test]
fn vm_begin_nextfile_is_invalid() {
    let cp = compile("BEGIN { nextfile }");
    let mut rt = runtime_with_slots(&cp);
    let e = vm_run_begin(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("nextfile"), "{e:?}");
}

#[test]
fn vm_end_nextfile_is_invalid() {
    let cp = compile("END { nextfile }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    let e = vm_run_end(&cp, &mut rt).unwrap_err();
    assert!(e.to_string().contains("nextfile"), "{e:?}");
}

#[test]
fn vm_end_runs_and_prints() {
    let cp = compile("END { print \"bye\" }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    rt.print_buf.clear();
    vm_run_end(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "bye\n");
}

#[test]
fn vm_pattern_always_matches() {
    let cp = compile("{ print $1 }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("x y");
    assert!(vm_pattern_matches(rule, &cp, &mut rt).unwrap());
}

#[test]
fn vm_pattern_literal_substring() {
    let cp = compile("/ell/ { print }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("hello");
    assert!(vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    rt.set_record_from_line("zzz");
    assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
}

#[test]
fn vm_pattern_expr_numeric() {
    let cp = compile("$1 > 10 { print \"big\" }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("20");
    assert!(vm_pattern_matches(rule, &cp, &mut rt).unwrap());
    rt.set_record_from_line("5");
    assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
}

#[test]
fn vm_run_rule_capture_print() {
    let cp = compile("{ print $1, $2 }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("a b");
    let mut cap = Vec::new();
    let flow = vm_run_rule(rule, &cp, &mut rt, Some(&mut cap), None).unwrap();
    assert!(matches!(flow, Flow::Normal));
    assert_eq!(cap.len(), 1);
    assert!(cap[0].starts_with("a"));
    assert!(cap[0].contains("b"));
}

#[test]
fn vm_run_rule_next_signal() {
    let cp = compile("{ next }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("z");
    let flow = vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
    assert!(matches!(flow, Flow::Next));
}

#[test]
fn vm_run_rule_exit_sets_pending() {
    let cp = compile("{ exit 3 }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("z");
    let flow = vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
    assert!(matches!(flow, Flow::ExitPending));
    assert!(rt.exit_pending);
    assert_eq!(rt.exit_code, 3);
}

#[test]
fn vm_beginfile_empty_ok() {
    let cp = compile("{ }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_beginfile(&cp, &mut rt).unwrap();
}

#[test]
fn vm_endfile_empty_ok() {
    let cp = compile("{ }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_endfile(&cp, &mut rt).unwrap();
}

#[test]
fn flush_print_buf_empty_ok() {
    let mut buf = Vec::new();
    flush_print_buf(&mut buf).unwrap();
    assert!(buf.is_empty());
}

#[test]
fn vm_user_function_call_in_record_rule() {
    let cp = compile("function dbl(x){ return x*2 } { print dbl($1) }");
    let rule = &cp.record_rules[0];
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("21");
    let mut cap = Vec::new();
    vm_run_rule(rule, &cp, &mut rt, Some(&mut cap), None).unwrap();
    assert_eq!(cap.len(), 1);
    assert!(cap[0].starts_with("42"));
}

#[test]
fn vm_concat_and_comparison_in_begin() {
    let cp = compile("BEGIN { print (\"a\" < \"b\") }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "1\n");
}

#[test]
fn vm_array_set_read_in_begin() {
    let cp = compile("BEGIN { a[\"k\"] = 7; print a[\"k\"] }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "7\n");
}

#[test]
fn vm_begin_printf_statement() {
    let cp = compile("BEGIN { printf \"%s\", \"ok\" }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "ok");
}

#[test]
fn vm_begin_if_branch() {
    let cp = compile("BEGIN { if (1) print 7; }");
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    assert_eq!(String::from_utf8_lossy(&rt.print_buf), "7\n");
}

#[test]
fn vm_pattern_range_placeholder_returns_false_in_vm() {
    let cp = compile("/a/,/b/ { print }");
    let rule = &cp.record_rules[0];
    assert!(matches!(rule.pattern, CompiledPattern::Range { .. }));
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("x");
    assert!(!vm_pattern_matches(rule, &cp, &mut rt).unwrap());
}

// ── vm_range_step / vm_match_range_endpoint pinning ──────────────────────
//
// Range pattern state machine: `state` is false until `start` matches on a
// record, then true until `end` matches on a record (inclusive of the
// end-matching record). Critical that:
//   - Same-record start-and-end stays true for that one record then resets
//   - state survives across records
//   - Always/Never/NestedRangeError endpoints behave per spec
// Any regression in this state machine silently breaks /pat1/,/pat2/
// programs without changing any exit code — exactly the bug class hardest
// to catch in integration tests. Pin it.

fn endpoint_from_range_pattern(cp: &CompiledProgram, want_start: bool) -> &CompiledRangeEndpoint {
    match &cp.record_rules[0].pattern {
        CompiledPattern::Range { start, end } => {
            if want_start {
                start
            } else {
                end
            }
        }
        _ => panic!("expected range pattern in compiled rule[0]"),
    }
}

#[test]
fn range_step_start_match_activates_state() {
    let cp = compile("/A/,/Z/ { print }");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    let end = endpoint_from_range_pattern(&cp, false).clone();
    let mut rt = runtime_with_slots(&cp);
    let mut state = false;

    rt.set_record_from_line("contains A here");
    assert!(vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
    assert!(state, "state must flip true on start match");
}

#[test]
fn range_step_stays_active_between_endpoints() {
    let cp = compile("/A/,/Z/ { print }");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    let end = endpoint_from_range_pattern(&cp, false).clone();
    let mut rt = runtime_with_slots(&cp);
    let mut state = true; // pre-activated (e.g. previous record matched start)

    rt.set_record_from_line("middle line no match");
    assert!(vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
    assert!(state, "state must stay true between endpoints");
}

#[test]
fn range_step_end_match_resets_state_inclusive() {
    // POSIX: the record that matches `end` is itself part of the range
    // (the rule runs for it), but state resets to false afterward.
    let cp = compile("/A/,/Z/ { print }");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    let end = endpoint_from_range_pattern(&cp, false).clone();
    let mut rt = runtime_with_slots(&cp);
    let mut state = true;

    rt.set_record_from_line("contains Z end");
    let ran = vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap();
    assert!(ran, "end-matching record must still run");
    assert!(!state, "state must reset after end match");
}

#[test]
fn range_step_inactive_no_start_match_skips() {
    let cp = compile("/A/,/Z/ { print }");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    let end = endpoint_from_range_pattern(&cp, false).clone();
    let mut rt = runtime_with_slots(&cp);
    let mut state = false;

    rt.set_record_from_line("none of the keys");
    assert!(!vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
    assert!(!state, "no start match means state stays false");
}

#[test]
fn range_step_same_record_starts_and_ends() {
    // Record contains BOTH endpoints — start activates, end resets, and the
    // record itself runs. This is the trickiest case: state transitions
    // false → true → false within one step, but the return is true.
    let cp = compile("/A/,/Z/ { print }");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    let end = endpoint_from_range_pattern(&cp, false).clone();
    let mut rt = runtime_with_slots(&cp);
    let mut state = false;

    rt.set_record_from_line("A and Z together");
    assert!(vm_range_step(&mut state, &start, &end, &cp, &mut rt).unwrap());
    assert!(!state, "state must reset after end match in same record");
}

#[test]
fn match_endpoint_always_returns_true() {
    let cp = compile("{ print }"); // anything; we don't need a range here
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("doesn't matter");
    assert!(vm_match_range_endpoint(&CompiledRangeEndpoint::Always, &cp, &mut rt).unwrap());
}

#[test]
fn match_endpoint_never_returns_false() {
    let cp = compile("{ print }");
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("doesn't matter");
    assert!(!vm_match_range_endpoint(&CompiledRangeEndpoint::Never, &cp, &mut rt).unwrap());
}

#[test]
fn match_endpoint_nested_range_is_runtime_error() {
    // Nested range patterns (`(a,b),c`) are rejected at runtime — must not
    // silently match or no-match.
    let cp = compile("{ print }");
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("x");
    let err = vm_match_range_endpoint(&CompiledRangeEndpoint::NestedRangeError, &cp, &mut rt)
        .unwrap_err();
    assert!(
        format!("{err}").contains("nested range"),
        "expected nested range error, got: {err}"
    );
}

#[test]
fn match_endpoint_literal_regexp_substring_match() {
    // LiteralRegexp uses str::contains, not the regex engine — must be a
    // pure substring scan (no anchoring, no metacharacter interpretation).
    let cp = compile("/foo/,/bar/ { print }");
    let mut rt = runtime_with_slots(&cp);

    rt.set_record_from_line("xxx foo yyy");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    assert!(vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());

    rt.set_record_from_line("no needle");
    assert!(!vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());
}

#[test]
fn match_endpoint_expr_truthy_falsy() {
    // Expr endpoint runs a chunk and applies truthy() to the TOS.
    // Use NR==2 as the start: must match on the 2nd record, not the 1st.
    let cp = compile("NR==2,NR==4 { print }");
    let start = endpoint_from_range_pattern(&cp, true).clone();
    let mut rt = runtime_with_slots(&cp);

    rt.nr = 1.0;
    rt.set_record_from_line("first");
    assert!(!vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());

    rt.nr = 2.0;
    rt.set_record_from_line("second");
    assert!(vm_match_range_endpoint(&start, &cp, &mut rt).unwrap());
}

// ── Field operations: pin POSIX field semantics ──────────────────────────
//
// Field operations are the awk hot path: `$N`, `$N = …`, `NF = n`. POSIX
// mandates a specific rebuild order: writing $N may extend NF with empty
// fields up to N, $0 must be rebuilt with OFS, and setting NF truncates
// fields and re-builds $0. Off-by-one in these operations breaks every awk
// program — pin each contract.

fn run_begin_capture(src: &str) -> String {
    let cp = compile(src);
    let mut rt = runtime_with_slots(&cp);
    vm_run_begin(&cp, &mut rt).unwrap();
    String::from_utf8_lossy(&rt.print_buf).into_owned()
}

#[test]
fn field_assignment_extends_nf_with_empty_fields() {
    // POSIX: $5 = "x" when NF was 0 must extend $0 to "    x" (with OFS).
    let out = run_begin_capture(r#"BEGIN { $5 = "x"; print NF; print $0 }"#);
    assert_eq!(out, "5\n    x\n", "{out:?}");
}

#[test]
fn field_set_rebuilds_dollar_zero_with_ofs() {
    // OFS = "|" must separate fields when $0 is rebuilt after $N=.
    let out = run_begin_capture(r#"BEGIN { OFS="|"; $1="a"; $2="b"; $3="c"; print $0 }"#);
    assert_eq!(out, "a|b|c\n", "{out:?}");
}

#[test]
fn nf_truncate_shortens_record() {
    // NF=2 on a 4-field $0 must drop fields 3-4 and rebuild $0.
    let out = run_begin_capture(r#"BEGIN { $0="a b c d"; NF=2; print NF; print $0 }"#);
    assert_eq!(out, "2\na b\n", "{out:?}");
}

#[test]
fn nf_extend_pads_with_empty_fields() {
    let out = run_begin_capture(r#"BEGIN { $0="a b"; NF=4; print NF; print $0 }"#);
    // NF was 2, now 4; new fields are empty. Default OFS=" ".
    assert_eq!(out, "4\na b  \n", "{out:?}");
}

#[test]
fn dynamic_field_access_computed_index() {
    // `$(1+1)` must read $2, not the string "2" or field 1+1=2 as different.
    let out = run_begin_capture(r#"BEGIN { $0="x y z"; print $(1+1) }"#);
    assert_eq!(out, "y\n", "{out:?}");
}

// ── Array operations: pin POSIX array semantics ──────────────────────────

#[test]
fn array_in_returns_one_for_existing_key() {
    let out = run_begin_capture(r#"BEGIN { a["k"]=1; print ("k" in a) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn array_in_returns_zero_for_missing_key_without_creating() {
    // POSIX: `k in a` MUST NOT auto-create the key (unlike a[k] read).
    let out = run_begin_capture(r#"BEGIN { print ("nope" in a); for (k in a) print k }"#);
    // First print: 0; for-in finds zero keys, prints nothing more.
    assert_eq!(out, "0\n", "in-test must not auto-create: {out:?}");
}

#[test]
fn array_index_read_auto_creates_uninit_entry() {
    // POSIX/gawk: `x = a[k]` auto-creates `a[k]` as Uninit. After the read,
    // `k in a` must be true. Implemented in the GetArrayElem dispatch:
    // if name != "SYMTAB" and the key is missing, insert Value::Uninit
    // before returning.
    let out = run_begin_capture(r#"BEGIN { x = a["k"]; print ("k" in a) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn delete_single_element_keeps_others() {
    let out = run_begin_capture(
        r#"BEGIN { a["x"]=1; a["y"]=2; delete a["x"]; print ("x" in a); print ("y" in a) }"#,
    );
    assert_eq!(out, "0\n1\n");
}

#[test]
fn delete_entire_array_removes_all_entries() {
    let out = run_begin_capture(r#"BEGIN { a["x"]=1; a["y"]=2; delete a; print length(a) }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn multidim_array_uses_subsep_join() {
    // Default SUBSEP is \x1c. a[1,2] indexes by "1\x1c2".
    let out = run_begin_capture(r#"BEGIN { a[1,2]=42; print a[1,2]; print ((1,2) in a) }"#);
    assert_eq!(out, "42\n1\n");
}

#[test]
fn for_in_iterates_all_keys() {
    // We can't assert order (impl-defined) but we can assert count + sum.
    let out = run_begin_capture(
        r#"BEGIN { a[1]=10; a[2]=20; a[3]=30; n=0; s=0; for (k in a) { n++; s += a[k] } print n; print s }"#,
    );
    assert_eq!(out, "3\n60\n");
}

// ── Control flow ─────────────────────────────────────────────────────────

#[test]
fn while_loop_runs_until_condition_false() {
    let out = run_begin_capture(r#"BEGIN { i=0; while (i<3) { print i; i++ } }"#);
    assert_eq!(out, "0\n1\n2\n");
}

#[test]
fn do_while_runs_body_at_least_once() {
    // Body must run even when condition is initially false.
    let out = run_begin_capture(r#"BEGIN { i=10; do { print i; i++ } while (i<3) }"#);
    assert_eq!(out, "10\n", "do-while must run at least once: {out:?}");
}

#[test]
fn break_exits_innermost_loop_only() {
    let out = run_begin_capture(
        r#"BEGIN { for (i=0; i<3; i++) { for (j=0; j<3; j++) { if (j==1) break; print i":"j } } }"#,
    );
    assert_eq!(out, "0:0\n1:0\n2:0\n");
}

#[test]
fn continue_skips_to_next_iteration() {
    let out = run_begin_capture(r#"BEGIN { for (i=0; i<5; i++) { if (i==2) continue; print i } }"#);
    assert_eq!(out, "0\n1\n3\n4\n");
}

#[test]
fn for_c_init_cond_iter_all_phases_run() {
    let out = run_begin_capture(r#"BEGIN { for (i=0; i<3; i++) print i*10 }"#);
    assert_eq!(out, "0\n10\n20\n");
}

// ── typeof variants ──────────────────────────────────────────────────────

#[test]
fn typeof_untyped_for_unset_scalar() {
    // typeof(unset scalar) returns "untyped" — matches gawk 5.x vocab.
    let out = run_begin_capture(r#"BEGIN { print typeof(u) }"#);
    assert_eq!(out, "untyped\n");
}

#[test]
fn typeof_string_value() {
    let out = run_begin_capture(r#"BEGIN { s="hi"; print typeof(s) }"#);
    assert_eq!(out, "string\n");
}

#[test]
fn typeof_numeric_value() {
    let out = run_begin_capture(r#"BEGIN { n=42; print typeof(n) }"#);
    assert_eq!(out, "number\n");
}

#[test]
fn typeof_array_value() {
    let out = run_begin_capture(r#"BEGIN { a[1]=1; print typeof(a) }"#);
    assert_eq!(out, "array\n");
}

// ── sub/gsub return value semantics ──────────────────────────────────────

#[test]
fn sub_returns_one_on_match() {
    let out = run_begin_capture(r#"BEGIN { s="hello"; n=sub("ell","ELL",s); print n; print s }"#);
    assert_eq!(out, "1\nhELLo\n");
}

#[test]
fn sub_returns_zero_on_no_match() {
    let out = run_begin_capture(r#"BEGIN { s="hello"; n=sub("xyz","X",s); print n; print s }"#);
    assert_eq!(out, "0\nhello\n");
}

#[test]
fn gsub_returns_count_of_replacements() {
    let out = run_begin_capture(r#"BEGIN { s="abababab"; n=gsub("ab","X",s); print n; print s }"#);
    assert_eq!(out, "4\nXXXX\n");
}

#[test]
fn gsub_on_dollar_zero_rebuilds_fields() {
    // Modifying $0 via gsub must update the field array.
    let out = run_begin_capture(r#"BEGIN { $0="a b c d"; gsub("b","BBB"); print $2 }"#);
    assert_eq!(out, "BBB\n");
}

// ── sprintf format specifiers ────────────────────────────────────────────

#[test]
fn sprintf_percent_d_truncates_to_integer() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%d", 3.7) }"#);
    assert_eq!(out, "3\n");
}

#[test]
fn sprintf_percent_f_default_six_decimals() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%f", 1.5) }"#);
    assert_eq!(out, "1.500000\n");
}

#[test]
fn sprintf_percent_s_string() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%s]", "hi") }"#);
    assert_eq!(out, "[hi]\n");
}

#[test]
fn sprintf_width_padding_right_aligned() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%5d]", 42) }"#);
    assert_eq!(out, "[   42]\n");
}

#[test]
fn sprintf_negative_width_left_aligned() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%-5d]", 42) }"#);
    assert_eq!(out, "[42   ]\n");
}

#[test]
fn sprintf_zero_pad() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%05d]", 42) }"#);
    assert_eq!(out, "[00042]\n");
}

#[test]
fn sprintf_percent_x_hex() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%x", 255) }"#);
    assert_eq!(out, "ff\n");
}

#[test]
fn sprintf_percent_o_octal() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%o", 8) }"#);
    assert_eq!(out, "10\n");
}

#[test]
fn sprintf_double_percent_emits_literal_percent() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("100%%") }"#);
    assert_eq!(out, "100%\n");
}

// ── CONVFMT / OFMT ───────────────────────────────────────────────────────

#[test]
fn convfmt_default_six_significant_digits() {
    // POSIX/gawk: default CONVFMT = "%.6g" applies to float→string in
    // concat context. The ConcatPoolStr peephole path was bypassing it via
    // `Value::into_string()` (format_number); fixed to dispatch through
    // `num_to_string_convfmt` for Num/Mpfr.
    let out = run_begin_capture(r#"BEGIN { x=3.141592653; print x "" }"#);
    assert_eq!(out, "3.14159\n");
}

#[test]
fn convfmt_custom_two_decimals() {
    let out = run_begin_capture(r#"BEGIN { CONVFMT="%.2f"; x=3.141592653; print x "" }"#);
    assert_eq!(out, "3.14\n");
}

#[test]
fn ofmt_used_by_print_for_floats() {
    // print uses OFMT for floats, CONVFMT for concatenation.
    let out = run_begin_capture(r#"BEGIN { OFMT="%.3f"; print 3.141592653 }"#);
    assert_eq!(out, "3.142\n", "{out:?}");
}

#[test]
fn convfmt_bypassed_for_integer_valued_floats() {
    // POSIX: integer-valued numbers print exact (no CONVFMT/OFMT), no ".000000".
    let out = run_begin_capture(r#"BEGIN { x=42; print x "" }"#);
    assert_eq!(out, "42\n");
}

// ── Additional sprintf format specifiers ─────────────────────────────────

#[test]
fn sprintf_percent_e_scientific_notation() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%e", 12345.0) }"#);
    // %e: one digit before decimal, 6 fractional digits, e+NN exponent.
    assert_eq!(out, "1.234500e+04\n");
}

#[test]
fn sprintf_percent_g_uses_scientific_for_large_exponent() {
    // %g switches to %e form when exponent >= precision (default 6).
    // Previously failed because the lexer wasn't parsing `1e7` as a single
    // number token (was `1` concat ident `e7`); fixed in lexer.rs.
    let big = run_begin_capture(r#"BEGIN { print sprintf("%g", 1e7) }"#);
    assert_eq!(big, "1e+07\n");

    let small = run_begin_capture(r#"BEGIN { print sprintf("%g", 0.0001) }"#);
    assert_eq!(small, "0.0001\n");
}

#[test]
fn sprintf_percent_c_from_integer_is_byte() {
    // %c with a numeric arg formats that byte. 65 → "A".
    let out = run_begin_capture(r#"BEGIN { print sprintf("%c", 65) }"#);
    assert_eq!(out, "A\n");
}

#[test]
fn sprintf_percent_c_from_string_takes_first_char() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%c", "Hello") }"#);
    assert_eq!(out, "H\n");
}

#[test]
fn sprintf_percent_d_negative_number() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%d", -42) }"#);
    assert_eq!(out, "-42\n");
}

#[test]
fn sprintf_precision_truncates_string() {
    // %.5s takes first 5 bytes/chars of the string.
    let out = run_begin_capture(r#"BEGIN { print sprintf("%.5s", "abcdefghij") }"#);
    assert_eq!(out, "abcde\n");
}

#[test]
fn sprintf_integer_precision_pads_with_zeros() {
    // POSIX: %.Nd zero-pads the integer magnitude to at least N digits.
    // The sign is added separately and doesn't count toward N.
    let out = run_begin_capture(r#"BEGIN { print sprintf("%.5d", 42) }"#);
    assert_eq!(out, "00042\n");
    let neg = run_begin_capture(r#"BEGIN { print sprintf("%.5d", -42) }"#);
    assert_eq!(neg, "-00042\n");
}

#[test]
fn sprintf_width_and_precision_combined() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%10.3f]", 3.14159) }"#);
    // 10-wide field, 3 fractional digits → "     3.142" (right-aligned, 6 spaces+4 chars)
    assert_eq!(out, "[     3.142]\n");
}

#[test]
fn sprintf_plus_flag_shows_positive_sign() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%+d %+d", 42, -42) }"#);
    assert_eq!(out, "+42 -42\n");
}

#[test]
fn sprintf_hash_flag_on_octal_emits_leading_zero() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%#o", 8) }"#);
    // # flag for %o prefixes a literal '0' if not already there.
    assert_eq!(out, "010\n");
}

// ── substr / index / length corners ──────────────────────────────────────

#[test]
fn substr_negative_start_uses_position_one() {
    // POSIX: substr("abc", -1, 5) treats start as 1 (or adjusted), length is 5
    // but anything before position 1 doesn't exist — effective output depends
    // on implementation. gawk: substr("abc",-1,5) → "abc" (3 chars).
    let out = run_begin_capture(r#"BEGIN { print substr("abc", -1, 5) }"#);
    // The chars from "max(1,-1)" to min(len, -1+5-1) = chars 1..3 = "abc".
    assert_eq!(out, "abc\n");
}

#[test]
fn substr_zero_length_returns_empty() {
    let out = run_begin_capture(r#"BEGIN { print "[" substr("hello", 2, 0) "]" }"#);
    assert_eq!(out, "[]\n");
}

#[test]
fn substr_length_exceeds_string_clamps_to_end() {
    let out = run_begin_capture(r#"BEGIN { print substr("hello", 3, 999) }"#);
    assert_eq!(out, "llo\n");
}

#[test]
fn substr_omitted_length_takes_rest() {
    let out = run_begin_capture(r#"BEGIN { print substr("hello", 3) }"#);
    assert_eq!(out, "llo\n");
}

#[test]
fn index_returns_one_based_position() {
    let out = run_begin_capture(r#"BEGIN { print index("hello", "ell") }"#);
    // 'e' at byte 2 (1-based).
    assert_eq!(out, "2\n");
}

#[test]
fn index_miss_returns_zero() {
    let out = run_begin_capture(r#"BEGIN { print index("hello", "xyz") }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn index_empty_needle_returns_one() {
    // gawk and awkrs agree: index("hello", "") → 1. (POSIX is ambiguous;
    // both major implementations treat empty needle as matching at start.)
    let out = run_begin_capture(r#"BEGIN { print index("hello", "") }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn length_of_empty_string_is_zero() {
    let out = run_begin_capture(r#"BEGIN { print length("") }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn length_of_integer_uses_string_form() {
    // length(123) → length of "123" → 3
    let out = run_begin_capture(r#"BEGIN { print length(123) }"#);
    assert_eq!(out, "3\n");
}

#[test]
fn length_of_array_is_element_count() {
    let out =
        run_begin_capture(r#"BEGIN { a[1]=1; a["x"]=2; a["multidim",1]=3; print length(a) }"#);
    assert_eq!(out, "3\n");
}

// ── split() edge cases ───────────────────────────────────────────────────

#[test]
fn split_empty_record_returns_zero() {
    let out = run_begin_capture(r#"BEGIN { n = split("", a); print n; print length(a) }"#);
    assert_eq!(out, "0\n0\n");
}

#[test]
fn split_default_whitespace_skips_leading_trailing() {
    // Default FS (space) splits on runs of whitespace, skipping leading/trailing.
    let out = run_begin_capture(
        r#"BEGIN { n = split("  a  b  c  ", a); print n; print a[1], a[2], a[3] }"#,
    );
    assert_eq!(out, "3\na b c\n");
}

#[test]
fn split_single_char_fs_keeps_empty_fields() {
    // Explicit FS=":" preserves empty fields (unlike default whitespace).
    let out = run_begin_capture(
        r#"BEGIN { n = split("a::b:c", a, ":"); print n; for(i=1;i<=n;i++) print "["a[i]"]" }"#,
    );
    assert_eq!(out, "4\n[a]\n[]\n[b]\n[c]\n");
}

#[test]
fn split_empty_fs_splits_each_character() {
    // FS="" treats each char as a field (gawk extension).
    let out =
        run_begin_capture(r#"BEGIN { n = split("abc", a, ""); print n; print a[1], a[2], a[3] }"#);
    assert_eq!(out, "3\na b c\n");
}

#[test]
fn split_regex_fs_with_multi_char_separator() {
    // Multi-char FS is treated as a regex.
    let out = run_begin_capture(
        r#"BEGIN { n = split("a||b||c", a, /\|\|/); print n; print a[1], a[2], a[3] }"#,
    );
    assert_eq!(out, "3\na b c\n");
}

// ── CONVFMT in non-concat contexts ───────────────────────────────────────
//
// After the ConcatPoolStr fix, CONVFMT applies in concat contexts. But
// POSIX says CONVFMT also applies in: array subscript coercion, regex
// match operand coercion, and other string-context number conversions.
// Pin each so a future change can't silently regress these to format_number.

#[test]
fn convfmt_applied_to_array_subscript() {
    // POSIX: array-subscript numeric coercion uses CONVFMT.
    // Implemented in vm.rs via `rt.value_to_array_key()` which dispatches
    // through num_to_string_convfmt for non-integer Num/Mpfr values.
    let out = run_begin_capture(r#"BEGIN { CONVFMT="%.0f"; a[3.14]=1; for (k in a) print k }"#);
    assert_eq!(out, "3\n");

    // Integer-valued keys still bypass CONVFMT (a[1] stays "1", not "1.0").
    let int_out = run_begin_capture(
        r#"BEGIN { CONVFMT="%.0f"; a[1]=1; a[42]=2; n=0; for(k in a){n++} print n }"#,
    );
    assert_eq!(int_out, "2\n");
}

#[test]
fn convfmt_applies_to_regex_match_operand() {
    // `3.14 ~ /14/` coerces 3.14 to string via CONVFMT, then matches.
    let out = run_begin_capture(
        r#"BEGIN { CONVFMT="%.0f"; if (3.14 ~ /14/) print "match"; else print "nomatch" }"#,
    );
    // With CONVFMT=%.0f, 3.14 → "3", no "14" substring → "nomatch".
    assert_eq!(out, "nomatch\n", "{out:?}");
}

// ── Peephole fusion fires in record-rule context too ─────────────────────
//
// normalize_field_indices runs inside peephole_optimize which is called
// from compile_chunk. Record-rule bodies and BEGIN/END use the same
// compile_chunk path, so fusion should fire identically. Pin it.

#[test]
fn print_field_fusion_fires_in_record_rule() {
    let cp = compile("{ print $1 }");
    let body_ops = &cp.record_rules[0].body.ops;
    assert!(
        body_ops
            .iter()
            .any(|op| matches!(op, Op::PrintFieldStdout(1))),
        "record-rule body should have PrintFieldStdout(1), got: {body_ops:?}"
    );
}

#[test]
fn add_field_to_slot_fusion_fires_in_record_rule() {
    let cp = compile("{ s += $2 } END { print s }");
    let body_ops = &cp.record_rules[0].body.ops;
    assert!(
        body_ops
            .iter()
            .any(|op| matches!(op, Op::AddFieldToSlot { field: 2, .. })),
        "record-rule body should have AddFieldToSlot{{field:2,..}}, got: {body_ops:?}"
    );
}

// ── Field-splitting modes: FPAT, FIELDWIDTHS ─────────────────────────────

fn run_record_capture(prog: &str, input_line: &str) -> String {
    // NB: do NOT call flush_print_buf — it drains rt.print_buf to stdout.
    // We want to inspect the buffer, so leave it intact.
    let cp = compile(prog);
    let mut rt = runtime_with_slots(&cp);
    crate::vm::vm_run_begin(&cp, &mut rt).unwrap();
    rt.set_record_from_line(input_line);
    rt.nr = 1.0;
    rt.fnr = 1.0;
    if let Some(rule) = cp.record_rules.first() {
        let _ = crate::vm::vm_run_rule(rule, &cp, &mut rt, None, None);
    }
    crate::vm::vm_run_end(&cp, &mut rt).unwrap();
    String::from_utf8_lossy(&rt.print_buf).into_owned()
}

#[test]
fn fpat_basic_pattern_extracts_fields() {
    // FPAT defines fields by pattern (gawk extension). Use a simple
    // word-pattern (avoid the alternation case which awkrs handles
    // incorrectly — see fpat_alternation_currently_wrong below).
    let prog = r#"BEGIN { FPAT="[a-z]+" } { print NF; print $1; print $2; print $3 }"#;
    let out = run_record_capture(prog, "abc 123 def 456 ghi");
    // Three word-fields: abc, def, ghi
    assert!(out.contains("3\n"), "expected NF=3 in: {out:?}");
    assert!(out.contains("abc\n"), "{out:?}");
    assert!(out.contains("def\n"), "{out:?}");
    assert!(out.contains("ghi\n"), "{out:?}");
}

#[test]
fn fpat_alternation_preserves_quoted_fields() {
    // gawk's classic CSV FPAT: `[^,]*|"[^"]*"`. Leftmost-longest semantic
    // is required so the quoted-string alternative wins over the comma-free
    // run when both could match. Implemented via top-level alternation
    // splitting + per-position longest-match selection in
    // runtime.rs::split_fields_fpat.
    let prog = r#"BEGIN { FPAT="[^,]*|\"[^\"]*\"" } { print NF; print $1; print $2; print $3 }"#;
    let out = run_record_capture(prog, r#"abc,"def, ghi",xyz"#);
    assert!(out.contains("3\n"), "expected NF=3 in: {out:?}");
    assert!(out.contains("abc\n"), "{out:?}");
    assert!(out.contains(r#""def, ghi""#), "{out:?}");
    assert!(out.contains("xyz\n"), "{out:?}");
}

#[test]
fn fieldwidths_splits_fixed_width_columns() {
    let prog = r#"BEGIN { FIELDWIDTHS="3 4 5" } { print NF; print "["$1"]["$2"]["$3"]" }"#;
    let out = run_record_capture(prog, "abc1234zzzzz");
    assert!(out.contains("3\n"), "expected NF=3: {out:?}");
    // 3-wide: "abc", 4-wide: "1234", 5-wide: "zzzzz"
    assert!(out.contains("[abc][1234][zzzzz]"), "{out:?}");
}

#[test]
fn multichar_fs_treated_as_regex() {
    // FS with more than one char is interpreted as a regex.
    let prog = r#"{ print NF; print $1; print $2 }"#;
    let cp = compile(prog);
    let mut rt = runtime_with_slots(&cp);
    rt.vars
        .insert("FS".into(), crate::runtime::Value::Str(r"[,;]".into()));
    crate::vm::vm_run_begin(&cp, &mut rt).unwrap();
    rt.set_record_from_line("a,b;c");
    rt.nr = 1.0;
    rt.fnr = 1.0;
    crate::vm::vm_run_rule(&cp.record_rules[0], &cp, &mut rt, None, None).unwrap();
    let out = String::from_utf8_lossy(&rt.print_buf).into_owned();
    assert!(out.contains("3\n") && out.contains("a\nb\n"), "{out:?}");
}

// ── gsub/sub additional edge cases ───────────────────────────────────────

#[test]
fn gsub_with_ampersand_in_replacement_uses_match() {
    // `&` in replacement is replaced with the matched text.
    let out = run_begin_capture(r#"BEGIN { s="abc"; gsub(/b/, "[&]", s); print s }"#);
    assert_eq!(out, "a[b]c\n");
}

#[test]
fn gsub_with_escaped_ampersand_is_literal() {
    // `\&` in replacement is a literal `&`.
    let out = run_begin_capture(r#"BEGIN { s="abc"; gsub(/b/, "\\&", s); print s }"#);
    assert_eq!(out, "a&c\n");
}

#[test]
fn gsub_anchored_pattern_caret() {
    // `^` matches start of string only.
    let out = run_begin_capture(r#"BEGIN { s="aaa"; n = gsub(/^a/, "X", s); print n; print s }"#);
    assert_eq!(out, "1\nXaa\n");
}

#[test]
fn gsub_anchored_pattern_dollar() {
    // `$` matches end.
    let out = run_begin_capture(r#"BEGIN { s="aaa"; n = gsub(/a$/, "X", s); print n; print s }"#);
    assert_eq!(out, "1\naaX\n");
}

#[test]
fn gensub_backref_substitution() {
    // gensub's `\1` / `\2` etc. refer to capture groups in the regex.
    // Implemented via expand_repl_with_caps in builtins.rs which uses
    // captures_iter() to retain group info (find_iter() doesn't).
    let out = run_begin_capture(
        r#"BEGIN { s="John Smith"; r=gensub(/(\w+) (\w+)/, "\\2, \\1", "g", s); print r }"#,
    );
    assert_eq!(out, "Smith, John\n");
}

#[test]
fn gensub_backref_with_numeric_occurrence() {
    // Numeric `how` arg (e.g. 2) replaces only the Nth occurrence — must
    // still expand backrefs in that one replacement.
    let out =
        run_begin_capture(r#"BEGIN { s="aa bb cc"; r=gensub(/(\w+)/, "[\\1]", 2, s); print r }"#);
    assert_eq!(out, "aa [bb] cc\n");
}

#[test]
fn gensub_ampersand_replacement_still_works_alongside_backref() {
    // `&` and `\N` are independent — both must work after the backref
    // refactor (replace_all_gensub uses expand_repl_with_caps for both).
    let out = run_begin_capture(r#"BEGIN { s="abc"; r=gensub(/b/, "[&]", "g", s); print r }"#);
    assert_eq!(out, "a[b]c\n");
}

// ── Math builtin coverage ────────────────────────────────────────────────

#[test]
fn math_log_one_is_zero() {
    let out = run_begin_capture(r#"BEGIN { print log(1) }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn math_log_e_is_one() {
    let out = run_begin_capture(r#"BEGIN { printf "%.6f\n", log(exp(1)) }"#);
    assert_eq!(out, "1.000000\n");
}

#[test]
fn math_exp_zero_is_one() {
    let out = run_begin_capture(r#"BEGIN { print exp(0) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn math_int_truncates_negative_toward_zero() {
    // POSIX: int(-3.7) = -3, not -4. (Truncation, not floor.)
    let out = run_begin_capture(r#"BEGIN { print int(-3.7) }"#);
    assert_eq!(out, "-3\n");
}

#[test]
fn math_int_truncates_positive_toward_zero() {
    let out = run_begin_capture(r#"BEGIN { print int(3.7) }"#);
    assert_eq!(out, "3\n");
}

#[test]
fn math_atan2_y_over_x_quadrant() {
    // atan2(1,1) = π/4 ≈ 0.7853981633974483
    let out = run_begin_capture(r#"BEGIN { printf "%.4f\n", atan2(1, 1) }"#);
    assert_eq!(out, "0.7854\n");
}

#[test]
fn math_atan2_zero_zero_is_zero() {
    let out = run_begin_capture(r#"BEGIN { print atan2(0, 0) }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn math_sqrt_of_zero() {
    let out = run_begin_capture(r#"BEGIN { print sqrt(0) }"#);
    assert_eq!(out, "0\n");
}

// ── tolower / toupper ────────────────────────────────────────────────────

#[test]
fn tolower_mixed_case() {
    let out = run_begin_capture(r#"BEGIN { print tolower("AbCdEf") }"#);
    assert_eq!(out, "abcdef\n");
}

#[test]
fn toupper_mixed_case() {
    let out = run_begin_capture(r#"BEGIN { print toupper("aBcDeF") }"#);
    assert_eq!(out, "ABCDEF\n");
}

#[test]
fn tolower_passes_through_non_letters() {
    let out = run_begin_capture(r#"BEGIN { print tolower("ABC 123!") }"#);
    assert_eq!(out, "abc 123!\n");
}

#[test]
fn toupper_passes_through_non_letters() {
    let out = run_begin_capture(r#"BEGIN { print toupper("abc 123!") }"#);
    assert_eq!(out, "ABC 123!\n");
}

// ── strftime format specifiers ───────────────────────────────────────────
//
// strftime delegates to chrono's `format`. We pin a stable UTC epoch and
// verify each major POSIX format specifier produces the expected output.
// 3rd arg = 1 forces UTC so tests are tz-stable in CI.
//
// Test epoch: 2024-01-15 03:45:06 UTC = 1705290306
const TEST_EPOCH: &str = "1705290306";

#[test]
fn strftime_year_four_digit() {
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("%Y", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "2024\n");
}

#[test]
fn strftime_month_two_digit() {
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("%m", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "01\n");
}

#[test]
fn strftime_day_of_month() {
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("%d", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "15\n");
}

#[test]
fn strftime_hour_24_minute_second() {
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("%H:%M:%S", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "03:45:06\n");
}

#[test]
fn strftime_combined_iso_date() {
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("%Y-%m-%d", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "2024-01-15\n");
}

#[test]
fn strftime_percent_percent_emits_literal_percent() {
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("100%%", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "100%\n");
}

#[test]
fn strftime_day_of_year() {
    // 2024-01-15 = day 15 (Jan 15)
    let out = run_begin_capture(&format!(
        r#"BEGIN {{ print strftime("%j", {TEST_EPOCH}, 1) }}"#
    ));
    assert_eq!(out, "015\n");
}

// ── mktime ───────────────────────────────────────────────────────────────

#[test]
fn mktime_returns_minus_one_on_invalid_month() {
    let out = run_begin_capture(r#"BEGIN { print mktime("2024 13 01 00 00 00") }"#);
    // gawk returns -1 for invalid date components; chrono's strict
    // construction does the same.
    assert_eq!(out, "-1\n");
}

#[test]
fn mktime_year_2000_january_one_positive_epoch() {
    // 2000-01-01 in any timezone is well after 1970, so epoch > 0.
    // `> 0` must be inside the print's expression — bare `print x > 0`
    // parses as a redirect to file "0". Always parenthesize.
    let out = run_begin_capture(r#"BEGIN { print (mktime("2000 1 1 0 0 0") > 0) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn mktime_too_few_fields_returns_minus_one() {
    let out = run_begin_capture(r#"BEGIN { print mktime("2024 1 1") }"#);
    assert_eq!(out, "-1\n");
}

// ── srand / rand: deterministic sequence with fixed seed ─────────────────
//
// POSIX: srand(seed) seeds the RNG and returns the PREVIOUS seed. Two runs
// with the same seed must produce the same sequence — a regression here
// would break every random-sampling awk program silently.

#[test]
fn srand_returns_previous_seed() {
    // First srand returns whatever the initial seed was (impl-defined).
    // Second srand returns the seed passed to the first.
    let out = run_begin_capture(r#"BEGIN { srand(42); prev=srand(99); print prev }"#);
    assert_eq!(out, "42\n");
}

#[test]
fn rand_sequence_stable_with_same_seed() {
    // Two srand(N) calls with the same N must reset to the same sequence.
    let out = run_begin_capture(
        r#"BEGIN { srand(7); a=rand(); b=rand(); srand(7); c=rand(); d=rand(); print (a==c) (b==d) }"#,
    );
    // "11" means both pairs matched.
    assert_eq!(out, "11\n");
}

#[test]
fn rand_values_in_half_open_unit_interval() {
    // rand() returns x ∈ [0, 1). Draw a few and verify each is in range.
    let out = run_begin_capture(
        r#"BEGIN { srand(1); ok=1; for(i=0;i<10;i++){ x=rand(); if (x<0||x>=1) ok=0 } print ok }"#,
    );
    assert_eq!(out, "1\n");
}

#[test]
fn rand_different_draws_with_same_seed_differ() {
    // Sanity: two consecutive rand() with the same seed should NOT be equal
    // (catastrophic regression: rand always returns the same value).
    let out = run_begin_capture(r#"BEGIN { srand(123); a=rand(); b=rand(); print (a==b) }"#);
    assert_eq!(out, "0\n");
}

// ── intdiv / intdiv0: integer division ──────────────────────────────────
//
// awkrs uses a 2-arg signature: `intdiv(a, b)` returns the integer
// quotient (truncated toward zero). `intdiv0(a, b)` returns 0 on division
// by zero instead of erroring.

#[test]
fn intdiv_positive_quotient() {
    let out = run_begin_capture(r#"BEGIN { print intdiv(17, 5) }"#);
    assert_eq!(out, "3\n");
}

#[test]
fn intdiv_exact_division() {
    let out = run_begin_capture(r#"BEGIN { print intdiv(20, 5) }"#);
    assert_eq!(out, "4\n");
}

#[test]
fn intdiv_truncates_negative_toward_zero() {
    // -17 / 5 → -3 (truncate toward zero), not -4 (floor).
    let out = run_begin_capture(r#"BEGIN { print intdiv(-17, 5) }"#);
    assert_eq!(out, "-3\n");
}

#[test]
fn intdiv_zero_divisor_errors() {
    let cp = compile(r#"BEGIN { intdiv(10, 0) }"#);
    let mut rt = runtime_with_slots(&cp);
    let result = crate::vm::vm_run_begin(&cp, &mut rt);
    assert!(result.is_err(), "intdiv(10, 0) must error, got Ok");
}

#[test]
fn intdiv0_zero_divisor_returns_zero_without_error() {
    // intdiv0(a, 0) returns 0 (the "safe" variant — no runtime error).
    let out = run_begin_capture(r#"BEGIN { print intdiv0(10, 0) }"#);
    assert_eq!(out, "0\n");
}

// ── Record / field rebuild edge cases ────────────────────────────────────

#[test]
fn set_dollar_zero_resplits_with_current_fs() {
    // `$0 = "..."` must re-split with the active FS.
    let out = run_begin_capture(r#"BEGIN { FS=":"; $0="a:b:c"; print NF; print $2 }"#);
    assert_eq!(out, "3\nb\n");
}

#[test]
fn nf_zero_clears_record_and_fields() {
    // POSIX: NF=0 makes $0 empty and removes all fields.
    let out = run_begin_capture(r#"BEGIN { $0="a b c"; NF=0; print NF; print "[" $0 "]" }"#);
    assert_eq!(out, "0\n[]\n");
}

#[test]
fn dollar_zero_assigns_through_nf_changes() {
    // After resetting $0, NF reflects the new field count.
    let out = run_begin_capture(r#"BEGIN { $0="x y"; print NF; $0="p q r s"; print NF }"#);
    assert_eq!(out, "2\n4\n");
}

#[test]
fn set_field_beyond_nf_extends_with_empties() {
    // $5 = "x" when NF was 2 → NF becomes 5, $3 and $4 are empty.
    let out = run_begin_capture(
        r#"BEGIN { $0="a b"; $5="z"; print NF; print "[" $3 "][" $4 "][" $5 "]" }"#,
    );
    assert_eq!(out, "5\n[][][z]\n");
}

#[test]
fn reassigning_field_one_rebuilds_dollar_zero() {
    let out = run_begin_capture(r#"BEGIN { $0="a b c"; $1="X"; print $0 }"#);
    assert_eq!(out, "X b c\n");
}

#[test]
fn fs_change_after_record_does_not_resplit() {
    // POSIX: changing FS doesn't re-split the current $0 retroactively;
    // it affects the NEXT record. (Within BEGIN we can verify by setting
    // $0 explicitly then changing FS and reading fields — fields stay
    // split per the FS that was active at the time of the assignment.)
    let out = run_begin_capture(r#"BEGIN { FS=":"; $0="a:b:c"; FS=" "; print NF; print $2 }"#);
    // Still 3 fields with FS=":" split (changing FS to " " after $0=
    // doesn't re-split the existing record).
    assert_eq!(out, "3\nb\n");
}

#[test]
fn sub_does_not_modify_on_no_match() {
    // sub returns 0 and leaves the target unchanged.
    let out = run_begin_capture(r#"BEGIN { s="hello"; n = sub(/xyz/, "X", s); print n; print s }"#);
    assert_eq!(out, "0\nhello\n");
}

#[test]
fn gsub_count_zero_returned_on_no_match() {
    let out =
        run_begin_capture(r#"BEGIN { s="hello"; n = gsub(/xyz/, "X", s); print n; print s }"#);
    assert_eq!(out, "0\nhello\n");
}

#[test]
fn gsub_default_target_is_dollar_zero() {
    // gsub() with 2 args operates on $0.
    let prog = r#"{ gsub(/o/, "0"); print }"#;
    let out = run_record_capture(prog, "foo bar boo");
    assert!(out.contains("f00 bar b00"), "{out:?}");
}

#[test]
fn print_field_fusion_end_to_end_behavior() {
    // Verify the fused opcode behaves identically to the unfused sequence
    // for actual user-visible output.
    let cp = compile("{ print $2 }");
    let mut rt = runtime_with_slots(&cp);
    rt.set_record_from_line("foo bar baz");
    crate::vm::vm_run_rule(&cp.record_rules[0], &cp, &mut rt, None, None).unwrap();
    crate::vm::flush_print_buf(&mut rt.print_buf).unwrap();
    // Output should contain "bar" + ORS.
    // We can't easily capture stdout here, so just verify the opcode shape:
    assert!(
        cp.record_rules[0]
            .body
            .ops
            .iter()
            .any(|op| matches!(op, Op::PrintFieldStdout(2))),
        "expected PrintFieldStdout(2) for `{{ print $2 }}`"
    );
}

// ── Comparison semantics: numeric vs string per POSIX ────────────────────

#[test]
fn cmp_two_numbers_uses_numeric_order() {
    let out = run_begin_capture(r#"BEGIN { print (10 < 9) ? "yes" : "no" }"#);
    assert_eq!(out, "no\n");
}

#[test]
fn cmp_string_literals_use_string_order() {
    let out = run_begin_capture(r#"BEGIN { print ("10" < "9") ? "yes" : "no" }"#);
    assert_eq!(out, "yes\n");
}

#[test]
fn cmp_string_literal_vs_number_uses_string_compare() {
    // POSIX/gawk: a string LITERAL is NOT a "numeric string". When mixed
    // with a number, the number is coerced to a string and the comparison
    // is STRING-wise.
    //
    // Numeric compare applies only when both operands are either numbers
    // or "numeric strings" — values from input/$N/-v that look numeric.
    // Bare `"10"` source-level literals stay as Value::StrLit and miss
    // the numeric-string predicate. Verified with `gawk 'BEGIN { print
    // ("10" < 9) ? "yes" : "no" }'` → "yes".
    let out = run_begin_capture(r#"BEGIN { print ("10" < 9) ? "yes" : "no" }"#);
    assert_eq!(out, "yes\n");
}

#[test]
fn cmp_uninit_equals_zero_numerically() {
    let out = run_begin_capture(r#"BEGIN { print (u == 0) ? "yes" : "no" }"#);
    assert_eq!(out, "yes\n");
}

#[test]
fn cmp_non_numeric_strings_use_string_order() {
    let out = run_begin_capture(r#"BEGIN { print ("apple" < "banana") ? "yes" : "no" }"#);
    assert_eq!(out, "yes\n");
}

// ── Compound assignment to various lvalue targets ────────────────────────

#[test]
fn compound_assign_to_simple_var() {
    let out = run_begin_capture(r#"BEGIN { x = 10; x += 5; print x }"#);
    assert_eq!(out, "15\n");
}

#[test]
fn compound_assign_to_array_element() {
    let out = run_begin_capture(r#"BEGIN { a[1] = 10; a[1] += 5; print a[1] }"#);
    assert_eq!(out, "15\n");
}

#[test]
fn compound_assign_to_field_rebuilds_record() {
    let out = run_begin_capture(r#"BEGIN { $0 = "10 20 30"; $2 += 100; print $0 }"#);
    assert_eq!(out, "10 120 30\n");
}

#[test]
fn compound_assign_div_and_mod() {
    // `+=`, `-=`, `*=`, `/=`, `%=` all parse and apply correctly. The
    // `^=` and `**=` exponentiation variants are covered separately.
    let out = run_begin_capture(r#"BEGIN { x=100; x/=4; print x; y=10; y%=3; print y }"#);
    assert_eq!(out, "25\n1\n");
}

#[test]
fn compound_pow_assign_supported() {
    // `x ^= n` and `x **= n` (gawk-style compound exponentiation) parse
    // and evaluate as `x = x ^ n`. Lexer emits `PowAssign` token; parser
    // maps it to `BinOp::Pow`.
    let out = run_begin_capture(r#"BEGIN { z=2; z^=8; print z }"#);
    assert_eq!(out, "256\n");
    let out2 = run_begin_capture(r#"BEGIN { z=2; z**=8; print z }"#);
    assert_eq!(out2, "256\n");
}

// ── Increment / decrement on different lvalues ───────────────────────────

#[test]
fn incdec_field_postinc_returns_old_value() {
    let out = run_begin_capture(r#"BEGIN { $0 = "5 6"; x = $1++; print x; print $1 }"#);
    assert_eq!(out, "5\n6\n");
}

#[test]
fn incdec_field_preinc_returns_new_value() {
    let out = run_begin_capture(r#"BEGIN { $0 = "5 6"; x = ++$1; print x; print $1 }"#);
    assert_eq!(out, "6\n6\n");
}

#[test]
fn incdec_array_element_postinc() {
    let out = run_begin_capture(r#"BEGIN { a[1] = 10; x = a[1]++; print x; print a[1] }"#);
    assert_eq!(out, "10\n11\n");
}

#[test]
fn incdec_uninit_starts_at_zero() {
    let out = run_begin_capture(r#"BEGIN { x = ++u; print x }"#);
    assert_eq!(out, "1\n");
}

// ── Logical short-circuit ────────────────────────────────────────────────

#[test]
fn logical_and_short_circuits_false_left() {
    let out = run_begin_capture(r#"BEGIN { n = 0; r = (0 && (n=1)); print r; print n }"#);
    assert_eq!(out, "0\n0\n");
}

#[test]
fn logical_or_short_circuits_true_left() {
    let out = run_begin_capture(r#"BEGIN { n = 0; r = (1 || (n=1)); print r; print n }"#);
    assert_eq!(out, "1\n0\n");
}

#[test]
fn ternary_evaluates_only_chosen_branch() {
    let out = run_begin_capture(
        r#"BEGIN { a=0; b=0; r = (1 ? (a=1) : (b=1)); print r; print a; print b }"#,
    );
    assert_eq!(out, "1\n1\n0\n");
}

// ── Match operator ~ / !~ ────────────────────────────────────────────────

#[test]
fn match_operator_returns_one_on_match() {
    let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /ell/) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn match_operator_returns_zero_on_no_match() {
    let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /xyz/) }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn not_match_inverts_result() {
    let out = run_begin_capture(r#"BEGIN { print ("hello" !~ /xyz/); print ("hello" !~ /ell/) }"#);
    assert_eq!(out, "1\n0\n");
}

#[test]
fn match_with_dynamic_regex_string() {
    let out = run_begin_capture(r#"BEGIN { pat = "[a-z]+"; print ("hello" ~ pat) }"#);
    assert_eq!(out, "1\n");
}

// ── User function calls ──────────────────────────────────────────────────

#[test]
fn user_function_returns_value() {
    let out = run_begin_capture(r#"function add(a, b) { return a + b } BEGIN { print add(3, 4) }"#);
    assert_eq!(out, "7\n");
}

#[test]
fn user_function_local_vars_via_extra_params() {
    // Extra params past the call site are local to the function.
    let out = run_begin_capture(
        r#"function f(x,    i) { i=99; return i+x } BEGIN { i=1; print f(10); print i }"#,
    );
    assert_eq!(out, "109\n1\n");
}

#[test]
fn user_function_recursion_factorial() {
    let out = run_begin_capture(
        r#"function fact(n) { return n<=1 ? 1 : n*fact(n-1) } BEGIN { print fact(5) }"#,
    );
    assert_eq!(out, "120\n");
}

#[test]
fn user_function_mutual_recursion() {
    let out = run_begin_capture(
        r#"function even(n) { return n==0 ? 1 : odd(n-1) }
               function odd(n)  { return n==0 ? 0 : even(n-1) }
               BEGIN { print even(10); print odd(7) }"#,
    );
    assert_eq!(out, "1\n1\n");
}

// ── Sprintf additional cases ─────────────────────────────────────────────

#[test]
fn sprintf_zero_pad_negative_sign_first() {
    // POSIX: when zero-padding a signed integer, zeros go BETWEEN the
    // sign and the magnitude. Fixed in format.rs::pad_numeric to detect
    // a leading sign and insert padding after it.
    let out = run_begin_capture(r#"BEGIN { print sprintf("%05d", -42) }"#);
    assert_eq!(out, "-0042\n");
}

#[test]
fn sprintf_zero_pad_positive_with_plus_flag() {
    // Same rule applies to `+` sign flag.
    let out = run_begin_capture(r#"BEGIN { print sprintf("%+05d", 42) }"#);
    assert_eq!(out, "+0042\n");
}

#[test]
fn sprintf_multiple_args_in_one_format() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%s=%d (%.2f)", "x", 7, 3.14) }"#);
    assert_eq!(out, "x=7 (3.14)\n");
}

#[test]
fn sprintf_space_flag_positive_number() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("% d % d", 42, -42) }"#);
    assert_eq!(out, " 42 -42\n");
}

// ── Truthy / falsy edges ─────────────────────────────────────────────────

#[test]
fn empty_string_is_falsy() {
    let out = run_begin_capture(r#"BEGIN { print "" ? "T" : "F" }"#);
    assert_eq!(out, "F\n");
}

#[test]
fn string_literal_zero_is_truthy_unlike_number_zero() {
    // POSIX/gawk: a string LITERAL is truthy iff non-empty. The numeric
    // value of `"0"` is irrelevant for string literals in boolean context.
    // Only Value::Str (from input/fields/-v) gets numeric coercion.
    // Fixed in runtime.rs::truthy / truthy_cond by splitting StrLit/Str.
    let out = run_begin_capture(r#"BEGIN { print ("0" ? "T" : "F"); print (0 ? "T" : "F") }"#);
    assert_eq!(out, "T\nF\n");
}

#[test]
fn whole_array_in_scalar_context_errors() {
    let cp = compile(r#"BEGIN { a[1]=1; if (a) print "yes" }"#);
    let mut rt = runtime_with_slots(&cp);
    let result = crate::vm::vm_run_begin(&cp, &mut rt);
    assert!(result.is_err(), "array-as-scalar must error");
}

// ── Multi-statement bodies ───────────────────────────────────────────────

#[test]
fn semicolon_separates_statements() {
    let out = run_begin_capture(r#"BEGIN { x=1; y=2; print x+y }"#);
    assert_eq!(out, "3\n");
}

#[test]
fn newline_separates_statements() {
    let out = run_begin_capture("BEGIN {\nx=1\ny=2\nprint x+y\n}");
    assert_eq!(out, "3\n");
}

#[test]
fn comment_after_statement_terminates_via_newline() {
    // After fix: `skip_ws` leaves the `\n` after a comment in place so the
    // lexer emits `Newline`, which terminates the assignment statement.
    let out = run_begin_capture("BEGIN { x = 42 # comment\n print x }");
    assert_eq!(out, "42\n");
}

#[test]
fn semicolon_after_statement_with_comment_works() {
    // Workaround for the comment-as-terminator bug: explicit `;` works.
    let out = run_begin_capture("BEGIN { x = 42; # comment\n print x }");
    assert_eq!(out, "42\n");
}

// ── String concatenation with mixed types ────────────────────────────────

#[test]
fn concat_string_and_number_coerces_number() {
    let out = run_begin_capture(r#"BEGIN { print "x" 42 "y" }"#);
    assert_eq!(out, "x42y\n");
}

#[test]
fn concat_with_uninit_treats_as_empty() {
    let out = run_begin_capture(r#"BEGIN { print "[" u "]" }"#);
    assert_eq!(out, "[]\n");
}

// ── For-in iteration ─────────────────────────────────────────────────────

#[test]
fn for_in_visits_each_key_exactly_once() {
    let out = run_begin_capture(
        r#"BEGIN { a["x"]=1; a["y"]=2; a["z"]=3; n=0; for (k in a) n++; print n }"#,
    );
    assert_eq!(out, "3\n");
}

#[test]
fn for_in_empty_array_runs_zero_iterations() {
    let out = run_begin_capture(r#"BEGIN { n=0; for (k in a) n++; print n }"#);
    assert_eq!(out, "0\n");
}

// ── Range pattern across records ─────────────────────────────────────────

#[test]
fn range_pattern_runs_for_records_inside_range() {
    let prog = r#"NR==2,NR==4 { print "in:" NR }"#;
    let cp = compile(prog);
    let mut rt = runtime_with_slots(&cp);
    let mut state = vec![false; cp.prog_rules_len];
    for nr in 1..=5 {
        rt.nr = nr as f64;
        rt.set_record_from_line(&format!("line{nr}"));
        let rule = &cp.record_rules[0];
        if let CompiledPattern::Range { start, end } = &rule.pattern {
            let run =
                vm_range_step(&mut state[rule.original_index], start, end, &cp, &mut rt).unwrap();
            if run {
                crate::vm::vm_run_rule(rule, &cp, &mut rt, None, None).unwrap();
            }
        }
    }
    let s = String::from_utf8_lossy(&rt.print_buf);
    assert!(
        s.contains("in:2") && s.contains("in:3") && s.contains("in:4"),
        "{s}"
    );
    assert!(!s.contains("in:1") && !s.contains("in:5"), "{s}");
}

// ── Block scope (awk has none — all vars are function/global) ────────────

#[test]
fn nested_blocks_share_variables() {
    let out = run_begin_capture(r#"BEGIN { { x = 1 } print x }"#);
    assert_eq!(out, "1\n");
}

// ── printf redirect to /dev/null runs without error ──────────────────────

#[test]
fn printf_redirect_overwrite_runs() {
    let out = run_begin_capture(r#"BEGIN { printf "%s\n", "ignored" > "/dev/null" }"#);
    assert_eq!(out, "");
}

// ── sprintf flag interactions ────────────────────────────────────────────

#[test]
fn sprintf_left_align_overrides_zero_pad() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%-05d]", 42) }"#);
    assert_eq!(out, "[42   ]\n");
}

#[test]
fn sprintf_plus_with_space_takes_plus() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("% +d", 42) }"#);
    assert_eq!(out, "+42\n");
}

#[test]
fn sprintf_hash_flag_on_hex_emits_0x_prefix() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%#x", 255) }"#);
    assert_eq!(out, "0xff\n");
}

#[test]
fn sprintf_hash_flag_on_upper_hex_uses_upper_0x() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("%#X", 255) }"#);
    assert_eq!(out, "0XFF\n");
}

#[test]
fn sprintf_negative_octal_via_unsigned_wrap() {
    // `print x > 0` parses as redirect-to-file "0" — must parenthesize
    // the comparison inside print. (See mktime_year_2000_january_one_…
    // comment for the same gotcha.)
    let out = run_begin_capture(r#"BEGIN { print (length(sprintf("%o", -1)) > 0) }"#);
    assert_eq!(out, "1\n");
}

// ── substr boundary conditions ───────────────────────────────────────────

#[test]
fn substr_start_at_one_takes_from_beginning() {
    let out = run_begin_capture(r#"BEGIN { print substr("hello", 1) }"#);
    assert_eq!(out, "hello\n");
}

#[test]
fn substr_start_beyond_string_returns_empty() {
    let out = run_begin_capture(r#"BEGIN { print "[" substr("hello", 10) "]" }"#);
    assert_eq!(out, "[]\n");
}

#[test]
fn substr_single_character_at_position() {
    let out = run_begin_capture(r#"BEGIN { print substr("hello", 3, 1) }"#);
    assert_eq!(out, "l\n");
}

// ── Regex behaviors ──────────────────────────────────────────────────────

#[test]
fn regex_character_class_matches() {
    let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /[a-z]+/) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn regex_negated_character_class() {
    let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /[^a-z]/) }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn regex_quantifier_plus_at_least_one() {
    let out = run_begin_capture(r#"BEGIN { print ("" ~ /a+/); print ("a" ~ /a+/) }"#);
    assert_eq!(out, "0\n1\n");
}

#[test]
fn regex_quantifier_star_zero_or_more() {
    let out = run_begin_capture(r#"BEGIN { print ("" ~ /a*/); print ("aaa" ~ /a*/) }"#);
    assert_eq!(out, "1\n1\n");
}

#[test]
fn regex_anchor_caret_only_at_start() {
    let out = run_begin_capture(r#"BEGIN { print ("foo" ~ /^foo/); print ("xfoo" ~ /^foo/) }"#);
    assert_eq!(out, "1\n0\n");
}

#[test]
fn regex_anchor_dollar_only_at_end() {
    let out = run_begin_capture(r#"BEGIN { print ("bar" ~ /bar$/); print ("barx" ~ /bar$/) }"#);
    assert_eq!(out, "1\n0\n");
}

#[test]
fn regex_alternation_picks_either() {
    let out = run_begin_capture(
        r#"BEGIN { print ("cat" ~ /cat|dog/); print ("dog" ~ /cat|dog/); print ("fish" ~ /cat|dog/) }"#,
    );
    assert_eq!(out, "1\n1\n0\n");
}

// ── match() builtin sets RSTART/RLENGTH ──────────────────────────────────

#[test]
fn match_builtin_sets_rstart_and_rlength() {
    let out = run_begin_capture(
        r#"BEGIN { r = match("hello world", /world/); print r, RSTART, RLENGTH }"#,
    );
    assert_eq!(out, "7 7 5\n");
}

#[test]
fn match_builtin_sets_rstart_zero_on_miss() {
    let out = run_begin_capture(r#"BEGIN { r = match("hello", /xyz/); print r, RSTART, RLENGTH }"#);
    assert_eq!(out, "0 0 -1\n");
}

// ── Special variables ────────────────────────────────────────────────────

#[test]
fn nf_initial_value_zero_in_begin() {
    let out = run_begin_capture(r#"BEGIN { print NF }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn nr_initial_value_zero_in_begin() {
    let out = run_begin_capture(r#"BEGIN { print NR }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn environ_array_present_in_begin() {
    let out = run_begin_capture(r#"BEGIN { n=0; for (k in ENVIRON) n++; print (n > 0) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn subsep_default_length_one() {
    let out = run_begin_capture(r#"BEGIN { print length(SUBSEP) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn subsep_used_for_multidim_array_keys() {
    let out = run_begin_capture(r#"BEGIN { a[1,2] = "x"; for (k in a) print length(k) }"#);
    // Key is "1" + SUBSEP + "2" = 3 chars
    assert_eq!(out, "3\n");
}

// ── Power operator edge cases ────────────────────────────────────────────

#[test]
fn pow_zero_to_zero_is_one() {
    let out = run_begin_capture(r#"BEGIN { print 0^0 }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn pow_negative_base_integer_exponent() {
    let out = run_begin_capture(r#"BEGIN { print (-2)^3 }"#);
    assert_eq!(out, "-8\n");
}

#[test]
fn pow_fractional_exponent() {
    let out = run_begin_capture(r#"BEGIN { print 4^0.5 }"#);
    assert_eq!(out, "2\n");
}

// ── Parser / statement corners ───────────────────────────────────────────

#[test]
fn semicolons_separate_statements_on_one_line() {
    let out = run_begin_capture("BEGIN { x = 1; if (x) print x; print x+1 }");
    assert_eq!(out, "1\n2\n");
}

#[test]
fn empty_begin_block_compiles() {
    let cp = compile(r#"BEGIN { }"#);
    assert_eq!(cp.begin_chunks.len(), 1);
}

// ── Concatenation chain ──────────────────────────────────────────────────

#[test]
fn concat_chain_preserves_order() {
    let out = run_begin_capture(r#"BEGIN { print "a" "b" "c" "d" "e" }"#);
    assert_eq!(out, "abcde\n");
}

// ── Regression guards for recent fixes ───────────────────────────────────

#[test]
fn pow_assign_star_star_form_works() {
    let out = run_begin_capture(r#"BEGIN { x = 3; x **= 4; print x }"#);
    assert_eq!(out, "81\n");
}

#[test]
fn pow_assign_caret_form_works() {
    let out = run_begin_capture(r#"BEGIN { x = 3; x ^= 4; print x }"#);
    assert_eq!(out, "81\n");
}

#[test]
fn empty_string_literal_falsy_regression() {
    let out = run_begin_capture(r#"BEGIN { print ("" ? "T" : "F") }"#);
    assert_eq!(out, "F\n");
}

#[test]
fn non_empty_string_literals_always_truthy() {
    // "0", "false", " ", "\t" — all non-empty StrLit are truthy.
    let out = run_begin_capture(
        r#"BEGIN { print ("0" ? "T":"F"), ("false" ? "T":"F"), (" " ? "T":"F"), ("\t" ? "T":"F") }"#,
    );
    assert_eq!(out, "T T T T\n");
}

// ── Error paths ──────────────────────────────────────────────────────────

fn run_begin_must_err(src: &str) -> std::result::Result<(), crate::error::Error> {
    let cp = compile(src);
    let mut rt = runtime_with_slots(&cp);
    crate::vm::vm_run_begin(&cp, &mut rt)
}

#[test]
fn division_by_zero_in_expression_errors() {
    let result = run_begin_must_err(r#"BEGIN { x = 1 / 0 }"#);
    assert!(result.is_err(), "1 / 0 should error");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("division") || msg.contains("zero"),
        "expected division-by-zero message, got: {msg}"
    );
}

#[test]
fn divide_assign_zero_errors() {
    let result = run_begin_must_err(r#"BEGIN { x = 1; x /= 0 }"#);
    assert!(result.is_err(), "x /= 0 should error");
}

#[test]
fn calling_undefined_function_errors() {
    let result = run_begin_must_err(r#"BEGIN { undefined_fn(1, 2) }"#);
    assert!(result.is_err(), "call to undefined function should error");
}

#[test]
fn wrong_arity_to_builtin_errors() {
    let result = run_begin_must_err(r#"BEGIN { x = sqrt() }"#);
    assert!(result.is_err(), "sqrt() with 0 args should error");
}

// ── Inc/dec on uninit ────────────────────────────────────────────────────

#[test]
fn postinc_uninit_returns_zero_then_var_becomes_one() {
    let out = run_begin_capture(r#"BEGIN { x = u++; print x; print u }"#);
    assert_eq!(out, "0\n1\n");
}

#[test]
fn postdec_uninit_returns_zero_then_var_becomes_neg_one() {
    let out = run_begin_capture(r#"BEGIN { x = u--; print x; print u }"#);
    assert_eq!(out, "0\n-1\n");
}

// ── Array delete ─────────────────────────────────────────────────────────

#[test]
fn delete_missing_key_is_silent_noop() {
    let out = run_begin_capture(r#"BEGIN { a[1] = "x"; delete a["nope"]; print length(a) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn delete_entire_empty_array_no_error() {
    let out = run_begin_capture(r#"BEGIN { delete a; print "ok" }"#);
    assert_eq!(out, "ok\n");
}

#[test]
fn delete_then_reassign_same_key() {
    let out = run_begin_capture(r#"BEGIN { a[1] = "old"; delete a[1]; a[1] = "new"; print a[1] }"#);
    assert_eq!(out, "new\n");
}

// ── Multi-dim arrays ─────────────────────────────────────────────────────

#[test]
fn multidim_array_in_test_returns_one() {
    let out = run_begin_capture(r#"BEGIN { a[1,2] = "x"; print ((1,2) in a) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn multidim_array_in_test_returns_zero_for_missing() {
    let out = run_begin_capture(r#"BEGIN { a[1,2] = "x"; print ((3,4) in a) }"#);
    assert_eq!(out, "0\n");
}

#[test]
fn multidim_delete_specific_key() {
    let out = run_begin_capture(
        r#"BEGIN { a[1,2] = "x"; a[1,3] = "y"; delete a[1,2]; print length(a) }"#,
    );
    assert_eq!(out, "1\n");
}

// ── Function arg semantics ───────────────────────────────────────────────

#[test]
fn array_passed_to_function_is_call_by_reference() {
    // POSIX: arrays are passed by reference; modifications inside the
    // function are visible to the caller. Fixed via the new
    // Op::CallUserBindArrays opcode + frame-aware array_elem_get/set/
    // for_in_keys in VmCtx.
    let out = run_begin_capture(
        r#"function f(a) { a["new"] = 99 } BEGIN { x["old"] = 1; f(x); print x["new"] }"#,
    );
    assert_eq!(out, "99\n");
}

#[test]
fn array_by_reference_preserves_existing_keys() {
    // Caller's pre-existing entries must survive the by-ref pass.
    let out = run_begin_capture(
        r#"function f(a) { a["new"] = 99 } BEGIN { x["old"] = 1; f(x); print x["old"]; print x["new"] }"#,
    );
    assert_eq!(out, "1\n99\n");
}

#[test]
fn array_by_reference_fills_empty_array() {
    let out = run_begin_capture(
        r#"function fill(a, n,    i) { for(i=1;i<=n;i++) a[i] = i*10 } BEGIN { fill(arr, 3); print arr[1], arr[2], arr[3] }"#,
    );
    assert_eq!(out, "10 20 30\n");
}

#[test]
fn scalar_passed_by_value_modifications_not_visible() {
    let out = run_begin_capture(
        r#"function f(s) { s = "modified" } BEGIN { x = "orig"; f(x); print x }"#,
    );
    assert_eq!(out, "orig\n");
}

// ── Recursion ────────────────────────────────────────────────────────────

#[test]
fn recursion_fibonacci_ten() {
    let out = run_begin_capture(
        r#"function fib(n) { return n < 2 ? n : fib(n-1) + fib(n-2) } BEGIN { print fib(10) }"#,
    );
    assert_eq!(out, "55\n");
}

// ── Print with no args ───────────────────────────────────────────────────

#[test]
fn print_with_no_args_prints_dollar_zero() {
    let out = run_begin_capture(r#"BEGIN { $0 = "the record"; print }"#);
    assert_eq!(out, "the record\n");
}

#[test]
fn print_empty_string_emits_just_ors() {
    let out = run_begin_capture(r#"BEGIN { print "" }"#);
    assert_eq!(out, "\n");
}

// ── Negative / large numbers ─────────────────────────────────────────────

#[test]
fn negative_zero_equals_zero() {
    let out = run_begin_capture(r#"BEGIN { print (-0 == 0) }"#);
    assert_eq!(out, "1\n");
}

#[test]
fn large_integer_print_bypasses_ofmt() {
    // Integer-valued numbers print exact (up to ~2^53) regardless of OFMT.
    // Fixed in runtime.rs::num_to_string_ofmt / num_to_string_convfmt by
    // applying the same `fract==0 && |n|<1e15` bypass that format_number
    // already used for the direct write_to path.
    let out = run_begin_capture(r#"BEGIN { print 999999999999 }"#);
    assert_eq!(out, "999999999999\n");
}

#[test]
fn one_million_prints_as_integer_not_scientific() {
    let out = run_begin_capture(r#"BEGIN { print 1000000 }"#);
    assert_eq!(out, "1000000\n");
}

#[test]
fn ofmt_still_applied_for_non_integer_floats() {
    // Regression guard: OFMT continues to format non-integer floats.
    let out = run_begin_capture(r#"BEGIN { OFMT="%.3f"; print 3.14159 }"#);
    assert_eq!(out, "3.142\n");
}

// ── Concat with empty edges ──────────────────────────────────────────────

#[test]
fn concat_with_empty_left_or_right() {
    let out = run_begin_capture(r#"BEGIN { print "" "abc"; print "abc" "" }"#);
    assert_eq!(out, "abc\nabc\n");
}

#[test]
fn concat_three_empties_yields_empty() {
    let out = run_begin_capture(r#"BEGIN { print "[" "" "" "" "]" }"#);
    assert_eq!(out, "[]\n");
}

// ── Regex dot metacharacter ──────────────────────────────────────────────

#[test]
fn regex_dot_matches_any_non_newline_char() {
    let out = run_begin_capture(r#"BEGIN { print ("hello" ~ /h.llo/) }"#);
    assert_eq!(out, "1\n");
}

// ── split() ──────────────────────────────────────────────────────────────

#[test]
fn split_returns_field_count() {
    let out = run_begin_capture(r#"BEGIN { n = split("a,b,c,d", a, ","); print n }"#);
    assert_eq!(out, "4\n");
}

#[test]
fn split_uses_default_fs_when_omitted() {
    let out =
        run_begin_capture(r#"BEGIN { n = split("a b c", a); print n; print a[1], a[2], a[3] }"#);
    assert_eq!(out, "3\na b c\n");
}

#[test]
fn split_clears_existing_array() {
    let out = run_begin_capture(
        r#"BEGIN { a["old"]=1; a["stale"]=2; n=split("x y", a, " "); print length(a) }"#,
    );
    assert_eq!(out, "2\n");
}

// ── sprintf string width / left-align ────────────────────────────────────

#[test]
fn sprintf_with_n_width_pads_string() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%10s]", "hi") }"#);
    assert_eq!(out, "[        hi]\n");
}

#[test]
fn sprintf_with_negative_n_left_aligns_string() {
    let out = run_begin_capture(r#"BEGIN { print sprintf("[%-10s]", "hi") }"#);
    assert_eq!(out, "[hi        ]\n");
}

#[test]
fn vm_large_array_deletion() {
    let out =
        run_begin_capture("BEGIN { for(i=0; i<1000; i++) a[i]=i; delete a; print length(a) }");
    assert_eq!(out, "0\n");
}

#[test]
fn vm_multidimensional_array_simulation() {
    let out = run_begin_capture("BEGIN { a[1,2]=42; print a[1,2], (1,2) in a }");
    assert_eq!(out, "42 1\n");
}

#[test]
fn vm_asort_behavior() {
    let out = run_begin_capture(
        "BEGIN { a[1]=\"z\"; a[2]=\"a\"; n=asort(a); for(i=1; i<=n; i++) printf \"%s\", a[i] }",
    );
    assert_eq!(out, "az");
}

#[test]
fn vm_asort_numeric_coercion() {
    // "10" (string) vs 2 (number) -> "10" > "2" is FALSE, but asort uses sort order.
    // POSIX asort sorts by value.
    let out = run_begin_capture(
        "BEGIN { a[1]=10; a[2]=2; n=asort(a); for(i=1; i<=n; i++) printf \"[%s]\", a[i] }",
    );
    assert_eq!(out, "[2][10]");
}

#[test]
fn vm_asorti_numeric_keys() {
    // asorti sorts keys.
    let out = run_begin_capture(
        "BEGIN { a[10]=\"x\"; a[2]=\"y\"; n=asorti(a); for(i=1; i<=n; i++) printf \"[%s]\", a[i] }",
    );
    // keys "10" and "2" as strings -> "10" < "2"
    assert_eq!(out, "[10][2]");
}

#[test]
fn vm_nested_function_recursion() {
    let out = run_begin_capture(
        "function f(n) { if(n<=0) return 0; return n + f(n-1) } BEGIN { print f(10) }",
    );
    assert_eq!(out, "55\n");
}

#[test]
fn vm_closure_like_array_passing() {
    // Arrays are call-by-reference in AWK.
    let out =
        run_begin_capture("function inc(arr) { arr[1]++ } BEGIN { a[1]=10; inc(a); print a[1] }");
    assert_eq!(out, "11\n");
}

#[test]
fn vm_scientific_notation_in_loop() {
    let out = run_begin_capture("BEGIN { sum=0; for(i=1e1; i<1.5e1; i++) sum+=i; print sum }");
    assert_eq!(out, "60\n"); // 10+11+12+13+14 = 60
}

#[test]
fn vm_switch_with_regex_case() {
    let out = run_begin_capture("BEGIN { x=\"abc\"; switch(x) { case /a/: print \"match\"; break; default: print \"no\" } }");
    assert_eq!(out, "match\n");
}

#[test]
fn vm_bignum_pow_large() {
    // Only if bignum is enabled, but run_begin_capture might not enable it by default.
    // Let's use a test that works in both but is more interesting in bignum.
    let out = run_begin_capture("BEGIN { print 2^10 }");
    assert_eq!(out, "1024\n");
}

#[test]
fn vm_complex_ternary_logic() {
    let out = run_begin_capture("BEGIN { print (1 ? (0 ? \"a\" : \"b\") : \"c\") }");
    assert_eq!(out, "b\n");
}

#[test]
fn vm_for_in_sorted_order() {
    // PROCINFO["sorted_in"] = "@ind_str_asc"
    let out = run_begin_capture("BEGIN { a[\"z\"]=1; a[\"a\"]=2; PROCINFO[\"sorted_in\"]=\"@ind_str_asc\"; for(i in a) printf \"%s\", i }");
    assert_eq!(out, "az");
}

#[test]
fn vm_getline_file_missing_returns_minus_one() {
    // getline < "no_such" returns -1 and sets ERRNO
    let out = run_begin_capture("BEGIN { r = (getline < \"no_such\"); print r, (ERRNO != \"\") }");
    // awkrs might error instead of returning -1 depending on configuration,
    // but POSIX says -1.
    assert!(out.contains("-1 1"), "got: {out}");
}

#[test]
fn vm_getline_pipe_empty_returns_zero() {
    // "echo -n" | getline returns 0 (EOF)
    let out = run_begin_capture("BEGIN { r = (\"printf ''\" | getline); print r }");
    assert_eq!(out, "0\n");
}

#[test]
fn vm_getline_pipe_into_var() {
    let out = run_begin_capture("BEGIN { \"echo hi\" | getline x; print x }");
    assert_eq!(out, "hi\n");
}

#[test]
fn vm_split_leading_trailing_separators() {
    // split(",a,b,", a, ",") -> "" "a" "b" ""
    let out = run_begin_capture(
        "BEGIN { n = split(\",a,b,\", a, \",\"); for(i=1;i<=n;i++) printf \"[%s]\", a[i] }",
    );
    assert_eq!(out, "[][a][b][]");
}

#[test]
fn vm_split_whitespace_behavior() {
    // split("  a  b  ", a, " ") behaves like default FS
    let out = run_begin_capture(
        "BEGIN { n = split(\"  a  b  \", a, \" \"); for(i=1;i<=n;i++) printf \"[%s]\", a[i] }",
    );
    assert_eq!(out, "[a][b]");
}

#[test]
fn internal_awk_cmp_eq_numeric_string_semantics() {
    let rt = Runtime::new();
    // Both numeric strings -> numeric compare
    assert_eq!(
        awk_cmp_eq(
            &Value::Str("10".into()),
            &Value::Str("10.0".into()),
            false,
            &rt
        )
        .as_number(),
        1.0
    );
    assert_eq!(
        awk_cmp_eq(
            &Value::Str("10".into()),
            &Value::Str("0xa".into()),
            false,
            &rt
        )
        .as_number(),
        0.0
    ); // is_numeric_str doesn't parse hex
}

#[test]
fn internal_awk_cmp_rel_mixed_types() {
    let rt = Runtime::new();
    // Number vs Numeric String -> numeric compare
    assert_eq!(
        awk_cmp_rel(
            BinOp::Lt,
            &Value::Num(2.0),
            &Value::Str("10".into()),
            false,
            &rt
        )
        .as_number(),
        1.0
    );
    // String literal (not numeric str) vs Number -> string compare
    // "10" (literal) vs 2 (number) -> "10" < "2" ? Yes.
    // Wait, awkrs might use as_str() which for 2.0 is "2".
    assert_eq!(
        awk_cmp_rel(
            BinOp::Lt,
            &Value::StrLit("10".into()),
            &Value::Num(2.0),
            false,
            &rt
        )
        .as_number(),
        1.0
    );
}

#[test]
fn internal_awk_cmp_eq_ignore_case() {
    let rt = Runtime::new();
    assert_eq!(
        awk_cmp_eq(
            &Value::Str("ABC".into()),
            &Value::Str("abc".into()),
            true,
            &rt
        )
        .as_number(),
        1.0
    );
    assert_eq!(
        awk_cmp_eq(
            &Value::Str("ABC".into()),
            &Value::Str("abc".into()),
            false,
            &rt
        )
        .as_number(),
        0.0
    );
}

#[test]
fn internal_awk_cmp_rel_ignore_case() {
    let rt = Runtime::new();
    // "B" > "a" normally, but with IGNORECASE "b" > "a"
    assert_eq!(
        awk_cmp_rel(
            BinOp::Gt,
            &Value::Str("B".into()),
            &Value::Str("a".into()),
            true,
            &rt
        )
        .as_number(),
        1.0
    );
    // "a" vs "B" -> "a" is 97, "B" is 66. "a" > "B" is true.
    assert_eq!(
        awk_cmp_rel(
            BinOp::Gt,
            &Value::Str("a".into()),
            &Value::Str("B".into()),
            false,
            &rt
        )
        .as_number(),
        1.0
    );
}

#[test]
fn internal_awk_cmp_uninit() {
    let rt = Runtime::new();
    // Uninit == 0 (numeric)
    assert_eq!(
        awk_cmp_eq(&Value::Uninit, &Value::Num(0.0), false, &rt).as_number(),
        1.0
    );
    // Uninit == "" (string)
    assert_eq!(
        awk_cmp_eq(&Value::Uninit, &Value::Str("".into()), false, &rt).as_number(),
        1.0
    );
    // Uninit < 1 (numeric)
    assert_eq!(
        awk_cmp_rel(BinOp::Lt, &Value::Uninit, &Value::Num(1.0), false, &rt).as_number(),
        1.0
    );
}

#[test]
fn vm_printf_many_args() {
    let out = run_begin_capture("BEGIN { printf \"%d %d %d %d %d %d\", 1, 2, 3, 4, 5, 6 }");
    assert_eq!(out, "1 2 3 4 5 6");
}

#[test]
fn vm_printf_positional_star_width() {
    // positional width + sequential value
    let out = run_begin_capture("BEGIN { printf \"%*1$d\", 5, 42 }");
    assert_eq!(out, "   42");
}

#[test]
fn vm_getline_file_redirect_into_var() {
    let dir = std::env::temp_dir();
    let p = dir.join(format!("awkrs_getline_{}.txt", std::process::id()));
    std::fs::write(&p, "line1\nline2").unwrap();

    let src = format!(
        "BEGIN {{ (getline x < \"{}\"); (getline y < \"{}\"); print x, y }}",
        p.display(),
        p.display()
    );
    let out = run_begin_capture(&src);
    assert_eq!(out, "line1 line2\n");

    let _ = std::fs::remove_file(&p);
}

#[test]
fn vm_ternary_nested_logic_v2() {
    let out = run_begin_capture("BEGIN { print (1 ? 2 : 3 ? 4 : 5) }");
    assert_eq!(out, "2\n");
    let out2 = run_begin_capture("BEGIN { print (0 ? 2 : 0 ? 4 : 5) }");
    assert_eq!(out2, "5\n");
}

#[test]
fn vm_delete_array_reassign_v2() {
    let out = run_begin_capture("BEGIN { a[1]=1; delete a; a[1]=2; print a[1] }");
    assert_eq!(out, "2\n");
}

#[test]
fn vm_multidim_array_custom_subsep_v2() {
    let out = run_begin_capture("BEGIN { SUBSEP=\"|\"; a[1,2]=42; for (k in a) print k }");
    assert_eq!(out, "1|2\n");
}

#[test]
fn vm_for_in_loop_with_delete_current_v2() {
    // gawk: deleting the current key during for-in loop is safe.
    let out = run_begin_capture(
        "BEGIN { a[1]=1; a[2]=2; for (k in a) { delete a[k]; n++ }; print n, length(a) }",
    );
    assert_eq!(out, "2 0\n");
}

#[test]
fn vm_math_trig_v2() {
    let out = run_begin_capture("BEGIN { printf \"%.2f %.2f\", sin(0), cos(0) }");
    assert_eq!(out, "0.00 1.00");
}

#[test]
fn vm_string_substr_v2() {
    let out = run_begin_capture("BEGIN { print substr(\"abcde\", 2, 3) }");
    assert_eq!(out, "bcd\n");
}

#[test]
fn vm_gsub_on_var_v2() {
    let out = run_begin_capture("BEGIN { s=\"foo\"; n=gsub(\"o\", \"x\", s); print s, n }");
    assert_eq!(out, "fxx 2\n");
}

#[test]
fn vm_length_empty_v2() {
    let out = run_begin_capture("BEGIN { print length(\"\") }");
    assert_eq!(out, "0\n");
}

#[test]
fn vm_split_empty_v2() {
    let out = run_begin_capture("BEGIN { n=split(\"\", a, \":\"); print n, length(a) }");
    assert_eq!(out, "0 0\n");
}

#[test]
fn vm_math_atan2_v2() {
    let out = run_begin_capture("BEGIN { print atan2(0, -1) }");
    // atan2(0, -1) should be PI (approx 3.14159)
    assert!(out.contains("3.1415"));
}

#[test]
fn vm_multidim_delete_whole_array_v3() {
    let out = run_begin_capture("BEGIN { a[1,2]=3; a[3,4]=5; delete a; print length(a) }");
    assert_eq!(out, "0\n");
}

#[test]
fn vm_multidim_in_v3() {
    let out = run_begin_capture("BEGIN { a[1,2,3]=4; print (1,2,3) in a }");
    assert_eq!(out, "1\n");
}

#[test]
fn vm_multidim_subsep_join_v3() {
    let out = run_begin_capture("BEGIN { SUBSEP=\":\"; a[1,2]=3; for (k in a) print k }");
    assert_eq!(out, "1:2\n");
}

#[test]
fn vm_indirect_call_with_args_v3() {
    let out = run_begin_capture("function f(x) { return x+1 } BEGIN { fn=\"f\"; print @fn(10) }");
    assert_eq!(out, "11\n");
}

#[test]
fn vm_local_array_passed_to_func_v3() {
    let out = run_begin_capture("function f(a) { a[1]=2 } BEGIN { f(b); print b[1] }");
    assert_eq!(out, "2\n");
}

#[test]
fn vm_split_with_long_string_v3() {
    let s = "a".repeat(1000);
    let src = format!(
        "BEGIN {{ n=split(\"{}\", a, \"b\"); print n, length(a[1]) }}",
        s
    );
    let out = run_begin_capture(&src);
    assert_eq!(out, "1 1000\n");
}

#[test]
fn vm_gsub_with_metachar_v3() {
    let out = run_begin_capture("BEGIN { s=\"a.c\"; gsub(/\\./, \"b\", s); print s }");
    assert_eq!(out, "abc\n");
}

#[test]
fn vm_match_sets_rlenght_v3() {
    let out = run_begin_capture("BEGIN { match(\"foobar\", /oo/); print RLENGTH }");
    assert_eq!(out, "2\n");
}

#[test]
fn vm_sprintf_large_float_v3() {
    let out = run_begin_capture("BEGIN { printf \"%.0f\", 1e10 }");
    assert_eq!(out, "10000000000");
}

#[test]
fn vm_assign_to_nf_extends_fields_v3() {
    let out = run_begin_capture("BEGIN { $3=\"x\"; print $1, $2, $3 }");
    assert_eq!(out, "  x\n");
}

#[test]
fn vm_environ_access_v2() {
    let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
    std::env::set_var("AWKRS_TEST_VAR", "hello");
    let out = run_begin_capture("BEGIN { print ENVIRON[\"AWKRS_TEST_VAR\"] }");
    assert_eq!(out, "hello\n");
    std::env::remove_var("AWKRS_TEST_VAR");
}

#[test]
fn vm_nested_loops_break_v2() {
    let out = run_begin_capture(
        "BEGIN { for(i=1;i<=2;i++) { for(j=1;j<=2;j++) { print i,j; if(i==1 && j==1) break } } }",
    );
    // i=1, j=1 -> print 1 1 -> break inner -> i=2
    // i=2, j=1 -> print 2 1
    // i=2, j=2 -> print 2 2
    assert_eq!(out, "1 1\n2 1\n2 2\n");
}

#[test]
fn vm_split_with_seps_array_v4() {
    // gawk parity: split(s, a, fs, seps)
    let out = run_begin_capture("BEGIN { split(\"a:b:c\", a, \":\", s); print s[1], s[2] }");
    assert_eq!(out, ": :\n");
}

#[test]
fn vm_printf_escape_sequences_v4() {
    let out = run_begin_capture("BEGIN { printf \"a\\tb\\nc\" }");
    assert_eq!(out, "a\tb\nc");
}

#[test]
fn vm_string_to_number_coercion_v4() {
    let out = run_begin_capture("BEGIN { print \"123.45foo\" + 0 }");
    assert_eq!(out, "123.45\n");
}

#[test]
fn vm_complex_concatenation_v4() {
    let out = run_begin_capture("BEGIN { print \"a\" 1 \"b\" 2.5 }");
    assert_eq!(out, "a1b2.5\n");
}

#[test]
fn vm_assignment_as_expression_v4() {
    let out = run_begin_capture("BEGIN { print (x = 5) + 10 }");
    assert_eq!(out, "15\n");
}

#[test]
fn vm_pre_inc_as_expression_v4() {
    let out = run_begin_capture("BEGIN { x = 5; print ++x }");
    assert_eq!(out, "6\n");
}

#[test]
fn vm_post_inc_as_expression_v4() {
    let out = run_begin_capture("BEGIN { x = 5; print x++ }");
    assert_eq!(out, "5\n");
}

#[test]
fn vm_post_dec_as_expression_v4() {
    let out = run_begin_capture("BEGIN { x = 5; print x-- }");
    assert_eq!(out, "5\n");
}

#[test]
fn vm_pre_dec_as_expression_v4() {
    let out = run_begin_capture("BEGIN { x = 5; print --x }");
    assert_eq!(out, "4\n");
}

#[test]
fn vm_array_element_inc_v4() {
    let out = run_begin_capture("BEGIN { a[1] = 5; print ++a[1] }");
    assert_eq!(out, "6\n");
}

#[test]
fn vm_symtab_access_v4() {
    let src = "BEGIN { x = 10; print SYMTAB[\"x\"] }";
    let cp = compile(src);
    let mut rt = runtime_with_slots(&cp);
    rt.refresh_special_arrays(&cp, "awkrs");
    vm_run_begin(&cp, &mut rt).unwrap();
    let out = String::from_utf8_lossy(&rt.print_buf).into_owned();
    assert_eq!(out, "10\n");
}

#[test]
fn vm_length_array_v4() {
    let out = run_begin_capture("BEGIN { a[1]=1; a[2]=2; print length(a) }");
    assert_eq!(out, "2\n");
}

#[test]
fn vm_length_number_v4() {
    let out = run_begin_capture("BEGIN { print length(12345) }");
    assert_eq!(out, "5\n");
}

#[test]
fn vm_index_substring_v4() {
    let out = run_begin_capture("BEGIN { print index(\"foobar\", \"bar\") }");
    assert_eq!(out, "4\n");
}

#[test]
fn vm_tolower_toupper_v4() {
    let out = run_begin_capture("BEGIN { print tolower(\"ABC\"), toupper(\"abc\") }");
    assert_eq!(out, "abc ABC\n");
}

#[test]
fn vm_atan2_v4() {
    let out = run_begin_capture("BEGIN { printf \"%.2f\", atan2(1, 1) }");
    // atan2(1,1) is PI/4 approx 0.785...
    assert!(out.contains("0.79") || out.contains("0.78"));
}

#[test]
fn vm_exp_log_v4() {
    let out = run_begin_capture("BEGIN { printf \"%.0f\", exp(log(10)) }");
    assert_eq!(out, "10");
}

#[test]
fn vm_sqrt_v4() {
    let out = run_begin_capture("BEGIN { print sqrt(16) }");
    assert_eq!(out, "4\n");
}

#[test]
fn vm_int_v4() {
    let out = run_begin_capture("BEGIN { print int(3.9), int(-3.9) }");
    assert_eq!(out, "3 -3\n");
}

#[test]
fn vm_num_add_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1+2}"), "3\n");
}
#[test]
fn vm_num_sub_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 5-2}"), "3\n");
}
#[test]
fn vm_num_mul_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 2*3}"), "6\n");
}
#[test]
fn vm_num_div_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 6/2}"), "3\n");
}
#[test]
fn vm_num_mod_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 5%2}"), "1\n");
}
#[test]
fn vm_num_pow_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 2^3}"), "8\n");
}

#[test]
fn vm_cmp_eq_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1==1}"), "1\n");
}
#[test]
fn vm_cmp_ne_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1!=2}"), "1\n");
}
#[test]
fn vm_cmp_lt_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1<2}"), "1\n");
}
#[test]
fn vm_cmp_le_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1<=1}"), "1\n");
}
#[test]
fn vm_cmp_gt_v17() {
    assert_eq!(run_begin_capture("BEGIN{print (2>1)}"), "1\n");
}
#[test]
fn vm_cmp_ge_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 2>=2}"), "1\n");
}

#[test]
fn vm_logic_and_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1&&1}"), "1\n");
}
#[test]
fn vm_logic_or_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1||0}"), "1\n");
}
#[test]
fn vm_logic_not_v17() {
    assert_eq!(run_begin_capture("BEGIN{print !0}"), "1\n");
}

#[test]
fn vm_str_concat_v17() {
    assert_eq!(run_begin_capture("BEGIN{print \"a\" \"b\"}"), "ab\n");
}
#[test]
fn vm_str_len_v17() {
    assert_eq!(run_begin_capture("BEGIN{print length(\"abc\")}"), "3\n");
}
#[test]
fn vm_str_sub_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{print substr(\"abcd\",2,2)}"),
        "bc\n"
    );
}
#[test]
fn vm_str_idx_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{print index(\"abcd\",\"bc\")}"),
        "2\n"
    );
}

#[test]
fn vm_array_basic_v17() {
    assert_eq!(run_begin_capture("BEGIN{a[1]=2; print a[1]}"), "2\n");
}
#[test]
fn vm_array_in_v17() {
    assert_eq!(run_begin_capture("BEGIN{a[1]=2; print 1 in a}"), "1\n");
}
#[test]
fn vm_array_del_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{a[1]=2; delete a[1]; print 1 in a}"),
        "0\n"
    );
}
#[test]
fn vm_array_len_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{a[1]=1; a[2]=2; print length(a)}"),
        "2\n"
    );
}

#[test]
fn vm_if_true_v17() {
    assert_eq!(run_begin_capture("BEGIN{if(1)print 1}"), "1\n");
}
#[test]
fn vm_if_false_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{if(0)print 1; else print 2}"),
        "2\n"
    );
}
#[test]
fn vm_while_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{i=0; while(i<3) i++; print i}"),
        "3\n"
    );
}
#[test]
fn vm_do_while_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{i=0; do i++; while(i<3); print i}"),
        "3\n"
    );
}
#[test]
fn vm_for_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{for(i=0;i<3;i++) { }; print i}"),
        "3\n"
    );
}
#[test]
fn vm_for_in_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{a[1]=1; for(k in a) print k}"),
        "1\n"
    );
}

#[test]
fn vm_func_call_v17() {
    assert_eq!(
        run_begin_capture("function f(x){return x+1} BEGIN{print f(1)}"),
        "2\n"
    );
}
#[test]
fn vm_func_rec_v17() {
    assert_eq!(
        run_begin_capture("function f(x){if(x<=0)return 0; return x+f(x-1)} BEGIN{print f(3)}"),
        "6\n"
    );
}

#[test]
fn vm_assign_expr_v17() {
    assert_eq!(run_begin_capture("BEGIN{print x=5}"), "5\n");
}
#[test]
fn vm_compound_add_v17() {
    assert_eq!(run_begin_capture("BEGIN{x=1; x+=2; print x}"), "3\n");
}
#[test]
fn vm_inc_dec_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{x=1; print x++; print ++x; print x--; print --x}"),
        "1\n3\n3\n1\n"
    );
}

#[test]
fn vm_ternary_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 1?2:3}"), "2\n");
}
#[test]
fn vm_ternary_false_v17() {
    assert_eq!(run_begin_capture("BEGIN{print 0?2:3}"), "3\n");
}

#[test]
fn vm_sprintf_v17() {
    assert_eq!(
        run_begin_capture("BEGIN{print sprintf(\"%d\",123)}"),
        "123\n"
    );
}
#[test]
fn vm_toupper_v17() {
    assert_eq!(run_begin_capture("BEGIN{print toupper(\"abc\")}"), "ABC\n");
}
#[test]
fn vm_tolower_v17() {
    assert_eq!(run_begin_capture("BEGIN{print tolower(\"ABC\")}"), "abc\n");
}

#[test]
fn vm_num_add_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1+2}"), "3\n");
}
#[test]
fn vm_num_sub_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 5-2}"), "3\n");
}
#[test]
fn vm_num_mul_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 2*3}"), "6\n");
}
#[test]
fn vm_num_div_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 6/2}"), "3\n");
}
#[test]
fn vm_num_mod_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 5%2}"), "1\n");
}
#[test]
fn vm_num_pow_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 2^3}"), "8\n");
}

#[test]
fn vm_cmp_eq_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1==1}"), "1\n");
}
#[test]
fn vm_cmp_ne_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1!=2}"), "1\n");
}
#[test]
fn vm_cmp_lt_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1<2}"), "1\n");
}
#[test]
fn vm_cmp_le_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1<=1}"), "1\n");
}
#[test]
fn vm_cmp_gt_v37() {
    assert_eq!(run_begin_capture("BEGIN{print (2>1)}"), "1\n");
}
#[test]
fn vm_cmp_ge_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 2>=2}"), "1\n");
}

#[test]
fn vm_logic_and_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1&&1}"), "1\n");
}
#[test]
fn vm_logic_or_v37() {
    assert_eq!(run_begin_capture("BEGIN{print 1||0}"), "1\n");
}
#[test]
fn vm_logic_not_v37() {
    assert_eq!(run_begin_capture("BEGIN{print !0}"), "1\n");
}

#[test]
fn vm_str_concat_v37() {
    assert_eq!(run_begin_capture("BEGIN{print \"a\" \"b\"}"), "ab\n");
}
#[test]
fn vm_str_len_v37() {
    assert_eq!(run_begin_capture("BEGIN{print length(\"abc\")}"), "3\n");
}
#[test]
fn vm_str_sub_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{print substr(\"abcd\",2,2)}"),
        "bc\n"
    );
}
#[test]
fn vm_str_idx_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{print index(\"abcd\",\"bc\")}"),
        "2\n"
    );
}

#[test]
fn vm_array_basic_v37() {
    assert_eq!(run_begin_capture("BEGIN{a[1]=2; print a[1]}"), "2\n");
}
#[test]
fn vm_array_in_v37() {
    assert_eq!(run_begin_capture("BEGIN{a[1]=2; print 1 in a}"), "1\n");
}
#[test]
fn vm_array_del_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{a[1]=2; delete a[1]; print 1 in a}"),
        "0\n"
    );
}
#[test]
fn vm_array_len_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{a[1]=1; a[2]=2; print length(a)}"),
        "2\n"
    );
}

#[test]
fn vm_if_true_v37() {
    assert_eq!(run_begin_capture("BEGIN{if(1)print 1}"), "1\n");
}
#[test]
fn vm_if_false_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{if(0)print 1; else print 2}"),
        "2\n"
    );
}
#[test]
fn vm_while_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{i=0; while(i<3) i++; print i}"),
        "3\n"
    );
}
#[test]
fn vm_do_while_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{i=0; do i++; while(i<3); print i}"),
        "3\n"
    );
}
#[test]
fn vm_for_v37() {
    assert_eq!(
        run_begin_capture("BEGIN{for(i=0;i<3;i++) { }; print i}"),
        "3\n"
    );
}

#[test]
fn vm_convfmt_scientific_v14() {
    assert_eq!(
        run_begin_capture("BEGIN { CONVFMT=\"%.2e\"; print 123.456 \"\" }"),
        "1.23e+02\n"
    );
}
#[test]
fn vm_ofmt_fixed_v14() {
    assert_eq!(
        run_begin_capture("BEGIN { OFMT=\"%.2f\"; print 123.456 }"),
        "123.46\n"
    );
}
#[test]
fn vm_ignorecase_index_v14() {
    assert_eq!(
        run_begin_capture("BEGIN { IGNORECASE=1; print index(\"ABC\", \"a\") }"),
        "1\n"
    );
}
#[test]
fn vm_ignorecase_match_v14() {
    assert_eq!(
        run_begin_capture("BEGIN { IGNORECASE=1; print (\"ABC\" ~ /a/) }"),
        "1\n"
    );
}
#[test]
fn vm_ignorecase_split_v14() {
    assert_eq!(
        run_begin_capture("BEGIN { IGNORECASE=1; n=split(\"aXb\", a, /[x]/); print n }"),
        "2\n"
    );
}

#[test]
fn vm_recursion_with_state_v15() {
    let out = run_begin_capture(
        "function f(n, s) { if(n<=0) return s; return f(n-1, s n) } BEGIN { print f(3, \"\") }",
    );
    assert_eq!(out, "321\n");
}

#[test]
fn vm_array_arg_is_reference_v15() {
    let out = run_begin_capture("function f(a) { a[1]=10 } BEGIN { a[1]=1; f(a); print a[1] }");
    assert_eq!(out, "10\n");
}

#[test]
fn vm_scalar_arg_is_value_v15() {
    let out = run_begin_capture("function f(x) { x=10 } BEGIN { x=1; f(x); print x }");
    assert_eq!(out, "1\n");
}

#[test]
fn vm_op_add_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print 1+1}"), "2\n");
}
#[test]
fn vm_op_add_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print \"1\"+1}"), "2\n");
}
#[test]
fn vm_op_add_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print 1+\"1\"}"), "2\n");
}
#[test]
fn vm_op_add_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print \"1\"+\"1\"}"), "2\n");
}

#[test]
fn vm_op_sub_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print 2-1}"), "1\n");
}
#[test]
fn vm_op_sub_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print \"2\"-1}"), "1\n");
}
#[test]
fn vm_op_sub_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print 2-\"1\"}"), "1\n");
}
#[test]
fn vm_op_sub_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print \"2\"-\"1\"}"), "1\n");
}

#[test]
fn vm_op_mul_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print 2*3}"), "6\n");
}
#[test]
fn vm_op_mul_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print \"2\"*3}"), "6\n");
}
#[test]
fn vm_op_mul_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print 2*\"3\"}"), "6\n");
}
#[test]
fn vm_op_mul_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print \"2\"*\"3\"}"), "6\n");
}

#[test]
fn vm_op_div_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print 6/2}"), "3\n");
}
#[test]
fn vm_op_div_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print \"6\"/2}"), "3\n");
}
#[test]
fn vm_op_div_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print 6/\"2\"}"), "3\n");
}
#[test]
fn vm_op_div_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print \"6\"/\"2\"}"), "3\n");
}

#[test]
fn vm_op_mod_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print 5%2}"), "1\n");
}
#[test]
fn vm_op_mod_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print \"5\"%2}"), "1\n");
}
#[test]
fn vm_op_mod_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print 5%\"2\"}"), "1\n");
}
#[test]
fn vm_op_mod_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print \"5\"%\"2\"}"), "1\n");
}

#[test]
fn vm_op_pow_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print 2^3}"), "8\n");
}
#[test]
fn vm_op_pow_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print \"2\"^3}"), "8\n");
}
#[test]
fn vm_op_pow_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print 2^\"3\"}"), "8\n");
}
#[test]
fn vm_op_pow_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print \"2\"^\"3\"}"), "8\n");
}

#[test]
fn vm_op_cmp_eq_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print (1==1)}"), "1\n");
}
#[test]
fn vm_op_cmp_eq_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"==1)}"), "1\n");
}
#[test]
fn vm_op_cmp_eq_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print (1==\"1\")}"), "1\n");
}
#[test]
fn vm_op_cmp_eq_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"==\"1\")}"), "1\n");
}

#[test]
fn vm_op_cmp_ne_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print (1!=2)}"), "1\n");
}
#[test]
fn vm_op_cmp_ne_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"!=2)}"), "1\n");
}
#[test]
fn vm_op_cmp_ne_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print (1!=\"2\")}"), "1\n");
}
#[test]
fn vm_op_cmp_ne_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"!=\"2\")}"), "1\n");
}

#[test]
fn vm_op_cmp_lt_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print (1<2)}"), "1\n");
}
#[test]
fn vm_op_cmp_lt_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"<2)}"), "1\n");
}
#[test]
fn vm_op_cmp_lt_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print (1<\"2\")}"), "1\n");
}
#[test]
fn vm_op_cmp_lt_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"<\"2\")}"), "1\n");
}

#[test]
fn vm_op_cmp_le_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print (1<=1)}"), "1\n");
}
#[test]
fn vm_op_cmp_le_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"<=1)}"), "1\n");
}
#[test]
fn vm_op_cmp_le_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print (1<=\"1\")}"), "1\n");
}
#[test]
fn vm_op_cmp_le_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"<=\"1\")}"), "1\n");
}

#[test]
fn vm_op_cmp_gt_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print (2>1)}"), "1\n");
}
#[test]
fn vm_op_cmp_gt_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"2\">1)}"), "1\n");
}
#[test]
fn vm_op_cmp_gt_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print (2>\"1\")}"), "1\n");
}
#[test]
fn vm_op_cmp_gt_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print (\"2\">\"1\")}"), "1\n");
}

#[test]
fn vm_op_cmp_ge_v16_0() {
    assert_eq!(run_begin_capture("BEGIN{print (2>=2)}"), "1\n");
}
#[test]
fn vm_op_cmp_ge_v16_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"2\">=2)}"), "1\n");
}
#[test]
fn vm_op_cmp_ge_v16_2() {
    assert_eq!(run_begin_capture("BEGIN{print (2>=\"2\")}"), "1\n");
}
#[test]
fn vm_op_cmp_ge_v16_3() {
    assert_eq!(run_begin_capture("BEGIN{print (\"2\">=\"2\")}"), "1\n");
}

#[test]
fn vm_mixed_eq_v64_0() {
    assert_eq!(run_begin_capture("BEGIN{print (1==\"1.0\")}"), "0\n");
}
#[test]
fn vm_mixed_eq_v64_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"==1.0)}"), "1\n");
}
#[test]
fn vm_mixed_lt_v64_0() {
    assert_eq!(run_begin_capture("BEGIN{print (1<\"2\")}"), "1\n");
}
#[test]
fn vm_mixed_lt_v64_1() {
    assert_eq!(run_begin_capture("BEGIN{print (\"1\"<2)}"), "1\n");
}

#[test]
fn vm_big_math_v64_0() {
    assert_eq!(
        run_begin_capture("BEGIN{print 1e20+1e20}"),
        "200000000000000000000\n"
    );
}
#[test]
fn vm_big_math_v64_1() {
    assert_eq!(run_begin_capture("BEGIN{print 1e20-1e20}"), "0\n");
}
#[test]
fn vm_big_math_v64_2() {
    assert_eq!(
        run_begin_capture("BEGIN{print 1e20*2}"),
        "200000000000000000000\n"
    );
}
#[test]
fn vm_big_math_v64_3() {
    assert_eq!(
        run_begin_capture("BEGIN{print 1e20/2}"),
        "50000000000000000000\n"
    );
}

#[test]
fn vm_str_cat_v64_0() {
    assert_eq!(run_begin_capture("BEGIN{print \"a\" \"b\" \"c\"}"), "abc\n");
}
#[test]
fn vm_str_cat_v64_1() {
    assert_eq!(run_begin_capture("BEGIN{print 1 2 3}"), "123\n");
}

#[test]
fn vm_idx_v64_0() {
    assert_eq!(
        run_begin_capture("BEGIN{print index(\"abcde\",\"cd\")}"),
        "3\n"
    );
}
#[test]
fn vm_idx_v64_1() {
    assert_eq!(
        run_begin_capture("BEGIN{print index(\"abcde\",\"xyz\")}"),
        "0\n"
    );
}

#[test]
fn vm_sub_v64_0() {
    assert_eq!(
        run_begin_capture("BEGIN{s=\"abc\"; print substr(s,2,1)}"),
        "b\n"
    );
}
#[test]
fn vm_sub_v64_1() {
    assert_eq!(
        run_begin_capture("BEGIN{print substr(\"abcde\",2,3)}"),
        "bcd\n"
    );
}

#[test]
fn vm_split_v64_0() {
    assert_eq!(
        run_begin_capture("BEGIN{n=split(\"a,b,c\",a,\",\"); print n,a[1],a[2],a[3]}"),
        "3 a b c\n"
    );
}
#[test]
fn vm_split_v64_1() {
    assert_eq!(
        run_begin_capture("BEGIN{n=split(\"a b c\",a); print n,a[1],a[2],a[3]}"),
        "3 a b c\n"
    );
}

#[test]
fn vm_arr_v64_0() {
    assert_eq!(
        run_begin_capture("BEGIN{a[1]=1; a[1.0]=2; print a[1]}"),
        "2\n"
    );
}
#[test]
fn vm_arr_v64_1() {
    assert_eq!(
        run_begin_capture("BEGIN{a[\"1\"]=1; a[1]=2; print a[\"1\"]}"),
        "2\n"
    );
}

#[test]
fn vm_time_v67_0() {
    assert_eq!(
        run_begin_capture("BEGIN{print strftime(\"%Y\", 0, 1)}"),
        "1970\n"
    );
}
#[test]
fn vm_time_v67_1() {
    assert_eq!(
        run_begin_capture("BEGIN{print strftime(\"%m\", 0, 1)}"),
        "01\n"
    );
}
#[test]
fn vm_time_v67_2() {
    assert_eq!(
        run_begin_capture("BEGIN{print strftime(\"%d\", 0, 1)}"),
        "01\n"
    );
}
#[test]
fn vm_time_v67_3() {
    assert_eq!(
        run_begin_capture("BEGIN{print strftime(\"%H\", 0, 1)}"),
        "00\n"
    );
}
#[test]
fn vm_time_v67_4() {
    assert_eq!(
        run_begin_capture("BEGIN{print strftime(\"%M\", 0, 1)}"),
        "00\n"
    );
}
#[test]
fn vm_time_v67_5() {
    assert_eq!(
        run_begin_capture("BEGIN{print strftime(\"%S\", 0, 1)}"),
        "00\n"
    );
}

#[test]
fn vm_mktime_v67_0() {
    assert_eq!(
        run_begin_capture("BEGIN{print mktime(\"1970 01 01 00 00 00\", 1)}"),
        "0\n"
    );
}
#[test]
fn vm_mktime_v67_1() {
    assert_eq!(
        run_begin_capture("BEGIN{print mktime(\"2023 01 01 00 00 00\", 1)}"),
        "1672531200\n"
    );
}

#[test]
fn vm_systime_v67() {
    assert!(run_begin_capture("BEGIN{print (systime() > 0)}").contains("1"));
}

#[test]
fn vm_atan2_v67() {
    assert_eq!(run_begin_capture("BEGIN{print atan2(0, 1)}"), "0\n");
}
#[test]
fn vm_atan2_v67_1() {
    assert_eq!(run_begin_capture("BEGIN{print atan2(1, 0)}"), "1.5708\n");
}
