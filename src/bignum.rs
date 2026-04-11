//! MPFR / `-M` helpers: integer truncation, strtonum, intdiv, and string forms without f64 loss.

use crate::error::{Error, Result};
use crate::runtime::{longest_f64_prefix, Runtime, Value};
use rug::float::Round;
use rug::Float;
use rug::Integer;

/// Parse a numeric string to MPFR without going through `f64` (so large integers and full `-M` paths stay consistent).
pub fn numeric_string_to_mpfr(s: &str, prec: u32, round: Round) -> Float {
    let t = s.trim();
    if t.is_empty() {
        return Float::with_val_round(prec, 0, round).0;
    }
    if t.starts_with("0x") || t.starts_with("0X") {
        return match Integer::from_str_radix(&t[2..], 16) {
            Ok(i) => Float::with_val_round(prec, i, round).0,
            Err(_) => Float::with_val_round(prec, 0, round).0,
        };
    }
    // gawk-style: octal only if no `8`/`9` in the digit run (else decimal, e.g. `01238`).
    if t.len() > 1
        && t.starts_with('0')
        && !t.starts_with("0x")
        && !t.starts_with("0X")
        && !t.contains('.')
        && !t.contains('e')
        && !t.contains('E')
        && t.bytes().all(|b| (b'0'..=b'7').contains(&b))
    {
        return match Integer::from_str_radix(t, 8) {
            Ok(i) => Float::with_val_round(prec, i, round).0,
            Err(_) => Float::with_val_round(prec, 0, round).0,
        };
    }
    let dec = longest_f64_prefix(t).unwrap_or("");
    if dec.is_empty() {
        return Float::with_val_round(prec, 0, round).0;
    }
    match Float::parse(dec) {
        Ok(ic) => Float::with_val_round(prec, ic, round).0,
        Err(_) => Float::with_val_round(prec, 0, round).0,
    }
}

/// Coerce any [`Value`] to MPFR for `-M` arithmetic / builtins (strings use [`numeric_string_to_mpfr`], not `parse_number`/`f64`).
pub fn value_to_mpfr(v: &Value, prec: u32, round: Round) -> Float {
    match v {
        Value::Mpfr(f) => f.clone(),
        Value::Num(n) => Float::with_val(prec, *n),
        Value::Str(s) | Value::StrLit(s) => numeric_string_to_mpfr(s, prec, round),
        Value::Regexp(s) => numeric_string_to_mpfr(s, prec, round),
        Value::Uninit => Float::with_val_round(prec, 0, round).0,
        Value::Array(_) => Float::with_val_round(prec, 0, round).0,
    }
}

/// Truncate toward zero as [`Integer`] (gawk-style integer ops).
pub fn float_trunc_integer(f: &Float) -> Integer {
    f.clone()
        .trunc()
        .to_integer_round(Round::Zero)
        .map(|(i, _)| i)
        .unwrap_or_else(|| Integer::from(0))
}

/// Bitwise operands use gawk’s unsigned-64 reinterpretation of the signed truncated value.
pub fn float_trunc_u64(f: &Float) -> u64 {
    float_trunc_integer(f).to_u64_wrapping()
}

pub fn awk_int_value(v: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(v.as_number().trunc());
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let f = value_to_mpfr(v, prec, round);
    Value::Mpfr(Float::with_val_round(prec, f.trunc(), round).0)
}

pub fn awk_intdiv_values(a: &Value, b: &Value, rt: &Runtime) -> Result<Value> {
    if !rt.bignum {
        let bf = b.as_number();
        if bf == 0.0 {
            return Err(Error::Runtime("intdiv: division by zero".into()));
        }
        let ai = a.as_number() as i64;
        let bi = bf as i64;
        return Ok(Value::Num((ai / bi) as f64));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let fa = value_to_mpfr(a, prec, round);
    let fb = value_to_mpfr(b, prec, round);
    if fb.is_zero() {
        return Err(Error::Runtime("intdiv: division by zero".into()));
    }
    let ia = float_trunc_integer(&fa);
    let ib = float_trunc_integer(&fb);
    if ib == 0 {
        return Err(Error::Runtime("intdiv: division by zero".into()));
    }
    let q = ia / ib;
    Ok(Value::Mpfr(Float::with_val_round(prec, q, round).0))
}

pub fn awk_strtonum_value(s: &str, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_strtonum(s));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    Value::Mpfr(numeric_string_to_mpfr(s, prec, round))
}

pub fn awk_and_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_and(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_mpfr(a, prec, round));
    let ub = float_trunc_u64(&value_to_mpfr(b, prec, round));
    let r = ua & ub;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_or_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_or(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_mpfr(a, prec, round));
    let ub = float_trunc_u64(&value_to_mpfr(b, prec, round));
    let r = ua | ub;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_xor_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_xor(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_mpfr(a, prec, round));
    let ub = float_trunc_u64(&value_to_mpfr(b, prec, round));
    let r = ua ^ ub;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_lshift_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_lshift(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let x = float_trunc_u64(&value_to_mpfr(a, prec, round));
    let n = float_trunc_u64(&value_to_mpfr(b, prec, round)) & 0x3f;
    let r = x << n;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_rshift_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_rshift(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let x = float_trunc_u64(&value_to_mpfr(a, prec, round));
    let n = float_trunc_u64(&value_to_mpfr(b, prec, round)) & 0x3f;
    let r = x >> n;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_compl_values(a: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_compl(a.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_mpfr(a, prec, round));
    let r = !ua;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

/// `%s` conversion for [`Float`]: exact integers as decimal digit strings (no MPFR fixed-point tail);
/// non-integers use MPFR’s string with trailing fractional zeros trimmed.
pub fn mpfr_string_for_percent_s(f: &Float) -> String {
    let tr = f.clone().trunc();
    if &tr == f {
        format!("{}", float_trunc_integer(f))
    } else {
        mpfr_string_trim_trailing_zeros(f.to_string())
    }
}

/// Strip redundant fractional zeros from MPFR’s default string (for `%s` / concat).
pub fn mpfr_string_trim_trailing_zeros(s: String) -> String {
    let mut t = s;
    if !t.contains('.') {
        return t;
    }
    while t.ends_with('0') {
        t.pop();
    }
    if t.ends_with('.') {
        t.pop();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::awk_sprintf_with_decimal;
    use crate::runtime::Runtime;
    use rug::float::Round;
    use std::str::FromStr;

    #[test]
    fn sprintf_percent_d_uses_integer_not_i64_clamp() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let i = Integer::from_str("9223372036854775808").unwrap(); // i64::MAX + 1
        let f = Float::with_val(rt.mpfr_prec_bits(), i);
        let s = awk_sprintf_with_decimal(
            "%d",
            &[Value::Mpfr(f)],
            '.',
            Some(','),
            Some((rt.mpfr_prec_bits(), Round::Nearest)),
        )
        .unwrap();
        assert_eq!(s, "9223372036854775808");
    }

    #[test]
    fn mpfr_percent_s_whole_number_is_plain_digits() {
        use std::str::FromStr;
        let i = Integer::from_str("1267650600228229401496703205376").unwrap();
        let f = Float::with_val(256, i);
        let s = mpfr_string_for_percent_s(&f);
        assert_eq!(s, "1267650600228229401496703205376");
        assert!(!s.contains('.'));
    }

    /// `i64::MAX + 1` must not round the augend through `f64` (would become 2^63 then +1 → 2^63+1).
    #[test]
    fn numeric_string_i64_max_plus_one_adds_exactly() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let a = numeric_string_to_mpfr("9223372036854775807", prec, round);
        let one = Float::with_val(prec, 1);
        let sum = Float::with_val_round(prec, &a + &one, round).0;
        let s = awk_sprintf_with_decimal(
            "%d",
            &[Value::Mpfr(sum)],
            '.',
            Some(','),
            Some((prec, round)),
        )
        .unwrap();
        assert_eq!(s, "9223372036854775808");
    }

    #[test]
    fn mpfr_string_trim_trailing_zeros_strips_dot_and_fractional_zeros() {
        assert_eq!(mpfr_string_trim_trailing_zeros("12.3400".into()), "12.34");
        assert_eq!(mpfr_string_trim_trailing_zeros("7.".into()), "7");
        assert_eq!(mpfr_string_trim_trailing_zeros("99".into()), "99");
    }

    #[test]
    fn mpfr_string_for_percent_s_non_integer_uses_trimmed_float_string() {
        let f = Float::with_val(64, 1.25);
        let s = mpfr_string_for_percent_s(&f);
        assert!(s.contains('2') && s.contains('5'), "{s}");
        assert!(s.contains('.'), "expected fractional form: {s}");
    }

    #[test]
    fn awk_intdiv_values_truncates_toward_zero_without_bignum() {
        let rt = Runtime::new();
        let q = awk_intdiv_values(&Value::Num(7.0), &Value::Num(2.0), &rt).unwrap();
        assert_eq!(q.as_number(), 3.0);
        let qn = awk_intdiv_values(&Value::Num(-7.0), &Value::Num(2.0), &rt).unwrap();
        assert_eq!(qn.as_number(), -3.0);
        let qd = awk_intdiv_values(&Value::Num(7.0), &Value::Num(-2.0), &rt).unwrap();
        assert_eq!(qd.as_number(), -3.0);
    }

    #[test]
    fn awk_intdiv_values_division_by_zero_errors() {
        let rt = Runtime::new();
        let e = awk_intdiv_values(&Value::Num(1.0), &Value::Num(0.0), &rt).unwrap_err();
        assert!(e.to_string().contains("intdiv"), "{e}");
    }

    #[test]
    fn awk_intdiv_values_bignum_integer_quotient() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let q = awk_intdiv_values(&Value::Num(10.0), &Value::Num(3.0), &rt).unwrap();
        let s = awk_sprintf_with_decimal(
            "%d",
            &[q],
            '.',
            Some(','),
            Some((rt.mpfr_prec_bits(), Round::Nearest)),
        )
        .unwrap();
        assert_eq!(s, "3");
    }

    #[test]
    fn numeric_string_to_mpfr_hex_prefix() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let f = numeric_string_to_mpfr("0x10", prec, round);
        let s =
            awk_sprintf_with_decimal("%d", &[Value::Mpfr(f)], '.', Some(','), Some((prec, round)))
                .unwrap();
        assert_eq!(s, "16");
    }

    #[test]
    fn numeric_string_to_mpfr_empty_trim_is_zero() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let f = numeric_string_to_mpfr("   ", prec, round);
        assert!(f.is_zero());
    }

    #[test]
    fn awk_int_value_truncates_float_without_bignum() {
        let rt = Runtime::new();
        let v = awk_int_value(&Value::Num(-9.7), &rt);
        assert_eq!(v.as_number(), -9.0);
    }

    #[test]
    fn float_trunc_u64_positive_integer() {
        let f = Float::with_val(64, 42.9);
        assert_eq!(float_trunc_u64(&f), 42u64);
    }

    #[test]
    fn float_trunc_integer_truncates_toward_zero() {
        let f = Float::with_val(64, -9.7);
        let i = float_trunc_integer(&f);
        assert_eq!(format!("{i}"), "-9");
    }

    #[test]
    fn awk_and_values_matches_builtin_bit_pattern_f64() {
        let rt = Runtime::new();
        let v = awk_and_values(&Value::Num(12.0), &Value::Num(10.0), &rt);
        assert_eq!(v.as_number(), crate::builtins::awk_and(12.0, 10.0));
    }

    #[test]
    fn awk_or_xor_values_match_builtins_f64() {
        let rt = Runtime::new();
        assert_eq!(
            awk_or_values(&Value::Num(8.0), &Value::Num(1.0), &rt).as_number(),
            crate::builtins::awk_or(8.0, 1.0)
        );
        assert_eq!(
            awk_xor_values(&Value::Num(15.0), &Value::Num(3.0), &rt).as_number(),
            crate::builtins::awk_xor(15.0, 3.0)
        );
    }

    #[test]
    fn awk_lshift_masks_shift_count_with_0x3f() {
        let rt = Runtime::new();
        // 65 & 0x3f == 1 → 1 << 1 == 2
        assert_eq!(
            awk_lshift_values(&Value::Num(1.0), &Value::Num(65.0), &rt).as_number(),
            2.0
        );
        // 64 & 0x3f == 0 →3 << 0 == 3
        assert_eq!(
            awk_lshift_values(&Value::Num(3.0), &Value::Num(64.0), &rt).as_number(),
            3.0
        );
    }

    #[test]
    fn awk_rshift_matches_builtin_f64() {
        let rt = Runtime::new();
        assert_eq!(
            awk_rshift_values(&Value::Num(16.0), &Value::Num(2.0), &rt).as_number(),
            crate::builtins::awk_rshift(16.0, 2.0)
        );
    }

    #[test]
    fn awk_compl_values_neg_one_to_zero_f64() {
        let rt = Runtime::new();
        assert_eq!(crate::builtins::awk_compl(-1.0), 0.0);
        assert_eq!(awk_compl_values(&Value::Num(-1.0), &rt).as_number(), 0.0);
    }

    fn mpfr_dec(v: &Value, rt: &Runtime) -> String {
        awk_sprintf_with_decimal(
            "%d",
            std::slice::from_ref(v),
            '.',
            Some(','),
            Some((rt.mpfr_prec_bits(), rt.mpfr_round())),
        )
        .unwrap()
    }

    #[test]
    fn awk_bitwise_bignum_path_agrees_with_f64_small_operands() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let a = Value::Num(12.0);
        let b = Value::Num(10.0);
        assert_eq!(mpfr_dec(&awk_and_values(&a, &b, &rt), &rt), "8");
        assert_eq!(mpfr_dec(&awk_or_values(&a, &b, &rt), &rt), "14");
        assert_eq!(mpfr_dec(&awk_xor_values(&a, &b, &rt), &rt), "6");
        assert_eq!(
            mpfr_dec(
                &awk_lshift_values(&Value::Num(3.0), &Value::Num(2.0), &rt),
                &rt
            ),
            "12"
        );
        assert_eq!(
            mpfr_dec(
                &awk_rshift_values(&Value::Num(17.0), &Value::Num(1.0), &rt),
                &rt
            ),
            "8"
        );
    }

    /// `compl(0)` uses the full **u64** bit pattern; `-M` keeps that exact integer for `%d` (unlike `f64`’s signed reinterpretation in scalar contexts).
    #[test]
    fn awk_compl_bignum_percent_d_is_full_u64_mask() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let v = awk_compl_values(&Value::Num(0.0), &rt);
        assert_eq!(mpfr_dec(&v, &rt), "18446744073709551615");
    }

    #[test]
    fn numeric_string_to_mpfr_leading_zero_octal_digits() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let f = numeric_string_to_mpfr("077", prec, round);
        assert_eq!(
            awk_sprintf_with_decimal("%d", &[Value::Mpfr(f)], '.', Some(','), Some((prec, round)))
                .unwrap(),
            "63"
        );
    }

    #[test]
    fn numeric_string_to_mpfr_zero_prefix_with_8_falls_back_decimal() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let f = numeric_string_to_mpfr("01238", prec, round);
        assert_eq!(
            awk_sprintf_with_decimal("%d", &[Value::Mpfr(f)], '.', Some(','), Some((prec, round)))
                .unwrap(),
            "1238"
        );
    }

    #[test]
    fn numeric_string_to_mpfr_invalid_hex_is_zero() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let prec = rt.mpfr_prec_bits();
        let round = rt.mpfr_round();
        let f = numeric_string_to_mpfr("0xzz", prec, round);
        assert!(f.is_zero());
    }

    #[test]
    fn awk_strtonum_value_hex_without_bignum_uses_builtin() {
        let rt = Runtime::new();
        let v = awk_strtonum_value("0x10", &rt);
        assert_eq!(v.as_number(), crate::builtins::awk_strtonum("0x10"));
    }

    #[test]
    fn awk_strtonum_value_empty_string_zero() {
        let rt = Runtime::new();
        assert_eq!(awk_strtonum_value("", &rt).as_number(), 0.0);
        let mut rtb = Runtime::new();
        rtb.bignum = true;
        assert!(awk_strtonum_value("", &rtb).as_number() == 0.0);
    }

    #[test]
    fn value_to_mpfr_uninit_and_empty_array_are_zero() {
        let prec = 64;
        let round = Round::Nearest;
        let u = value_to_mpfr(&Value::Uninit, prec, round);
        assert!(u.is_zero());
        let a = value_to_mpfr(
            &Value::Array(crate::runtime::AwkMap::default()),
            prec,
            round,
        );
        assert!(a.is_zero());
    }

    #[test]
    fn awk_strtonum_value_large_hex_integer_bignum_is_exact() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let v = awk_strtonum_value("0x10000000000000000", &rt);
        let s = mpfr_dec(&v, &rt);
        assert_eq!(s, "18446744073709551616");
    }

    #[test]
    fn mpfr_string_trim_trailing_zeros_all_fractional_zeros_becomes_int() {
        assert_eq!(mpfr_string_trim_trailing_zeros("7.000".into()), "7");
    }
}
