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
        Value::Str(s) | Value::StrLit(s) => numeric_string_to_mpfr(&s.to_str_lossy(), prec, round),
        Value::Regexp(s) => numeric_string_to_mpfr(&s.to_str_lossy(), prec, round),
        Value::Uninit => Float::with_val_round(prec, 0, round).0,
        Value::Array(_) => Float::with_val_round(prec, 0, round).0,
    }
}

/// A **literal**'s `f64` as MPFR, at the requested precision.
///
/// Widening the `f64` is not the same thing: `0.1` is already the nearest
/// double to one tenth, so `Float::with_val(prec, 0.1_f64)` produces that
/// double exactly — `0.1000000000000000055511151231257827…` — and ten additions
/// of it come to `1.00000000000000005551…` where gawk prints `1`. gawk parses
/// the literal's decimal text into MPFR instead, so it holds one tenth to the
/// requested precision.
///
/// The text is recovered rather than threaded through the bytecode: Rust's
/// `{}` for `f64` prints the shortest decimal that round-trips to the same
/// double, which for a literal a person wrote is the literal they wrote. That
/// keeps `Op::PushNum` carrying a plain `f64`, so the constant-folding
/// fusions that match on it are untouched. A value with no shorter spelling
/// (`1e300`, `2.5`) round-trips through the same path unchanged.
pub fn literal_f64_to_mpfr(n: f64, rt: &crate::runtime::Runtime) -> Float {
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    if n.is_finite() {
        numeric_string_to_mpfr(&format!("{n}"), prec, round)
    } else {
        Float::with_val_round(prec, n, round).0
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

/// `awk_int_value` — see implementation for the contract.
pub fn awk_int_value(v: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(v.as_number().trunc());
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let f = value_to_mpfr(v, prec, round);
    Value::Mpfr(Float::with_val_round(prec, f.trunc(), round).0)
}
/// `awk_intdiv_values` — see implementation for the contract.
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
/// `awk_strtonum_value` — see implementation for the contract.
pub fn awk_strtonum_value(s: &str, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_strtonum(s));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    Value::Mpfr(numeric_string_to_mpfr(s, prec, round))
}
/// gawk's three bitwise folds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitFold {
    /// `and(v1, v2, ...)`
    And,
    /// `or(v1, v2, ...)`
    Or,
    /// `xor(v1, v2, ...)`
    Xor,
}

impl BitFold {
    /// The fold a builtin name selects, if it names one.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "and" => Some(Self::And),
            "or" => Some(Self::Or),
            "xor" => Some(Self::Xor),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
        }
    }

    fn code(self) -> u8 {
        use fusevm::awk_host::bit_code;
        match self {
            Self::And => bit_code::AND,
            Self::Or => bit_code::OR,
            Self::Xor => bit_code::XOR,
        }
    }
}

/// `true` for a value below zero (`-0` and NaN are not), gawk's `mpfr_sgn < 0`.
fn mpfr_negative(f: &Float) -> bool {
    f.cmp0() == Some(std::cmp::Ordering::Less)
}

/// gawk's `%Rg` rendering of an operand in a `-M` fatal message.
fn mpfr_fatal_g(f: &Float) -> String {
    fusevm::awk_host::awk_fmt_g(f.to_f64())
}

/// gawk `and`/`or`/`xor` over every argument. `Err` carries gawk's fatal: fewer
/// than two arguments, or a negative operand — gawk pops the last argument
/// first, so the right-most negative one is reported.
///
/// Without `-M` this is gawk's 64-bit computation (shared with fusevm). With
/// `-M` gawk folds the truncated arbitrary-precision integers (`mpfr.c`
/// `do_mpfr_and`), so `or(2^70, 1)` is `2^70 + 1`, and words the fatal
/// `argument #N`.
pub fn awk_bit_fold_values(op: BitFold, args: &[Value], rt: &Runtime) -> Result<Value> {
    if !rt.bignum {
        let nums: Vec<f64> = args.iter().map(Value::as_number).collect();
        return fusevm::awk_host::awk_bit_fold_checked(op.code(), &nums)
            .map(Value::Num)
            .map_err(Error::Runtime);
    }
    if args.len() < 2 {
        return Err(Error::Runtime(format!(
            "{}: called with less than two arguments",
            op.name()
        )));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let vals: Vec<Float> = args.iter().map(|a| value_to_mpfr(a, prec, round)).collect();
    if let Some(i) = vals.iter().rposition(mpfr_negative) {
        return Err(Error::Runtime(format!(
            "{}: argument #{} negative value {} is not allowed",
            op.name(),
            i + 1,
            mpfr_fatal_g(&vals[i])
        )));
    }
    let mut acc = float_trunc_integer(&vals[0]);
    for f in &vals[1..] {
        let x = float_trunc_integer(f);
        match op {
            BitFold::And => acc &= x,
            BitFold::Or => acc |= x,
            BitFold::Xor => acc ^= x,
        }
    }
    Ok(Value::Mpfr(Float::with_val_round(prec, acc, round).0))
}

/// gawk `lshift(a, n)` (`left`) / `rshift(a, n)`. `Err` carries gawk's fatal
/// for a negative operand.
///
/// Without `-M`: 64-bit unsigned shift, a count of 64 or more yields 0, then
/// `adjust_uint` (shared with fusevm). With `-M` the integer shifts without a
/// width limit (`lshift(1, 70)` is `2^70`) and the fatal names the first
/// negative argument as `argument #N`.
pub fn awk_shift_values(left: bool, a: &Value, n: &Value, rt: &Runtime) -> Result<Value> {
    if !rt.bignum {
        return fusevm::awk_host::awk_shift_checked(left, a.as_number(), n.as_number())
            .map(Value::Num)
            .map_err(Error::Runtime);
    }
    let name = if left { "lshift" } else { "rshift" };
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let fa = value_to_mpfr(a, prec, round);
    let fn_ = value_to_mpfr(n, prec, round);
    for (i, f) in [&fa, &fn_].into_iter().enumerate() {
        if mpfr_negative(f) {
            return Err(Error::Runtime(format!(
                "{name}: argument #{} negative value {} is not allowed",
                i + 1,
                mpfr_fatal_g(f)
            )));
        }
    }
    let x = float_trunc_integer(&fa);
    let count = float_trunc_integer(&fn_).to_u32().unwrap_or(u32::MAX);
    let r = if left { x << count } else { x >> count };
    Ok(Value::Mpfr(Float::with_val_round(prec, r, round).0))
}

/// gawk `compl(a)`. `Err` carries gawk's fatal for a negative operand.
///
/// Without `-M` it is the 64-bit complement narrowed by `adjust_uint`
/// (`compl(0)` = `2^53 - 1`). With `-M` gawk complements the arbitrary-precision
/// integer (`compl(0)` = `-1`); its fatal reads `negative values` for an
/// integral operand (gawk's mpz path) and `negative value` otherwise.
pub fn awk_compl_values(a: &Value, rt: &Runtime) -> Result<Value> {
    if !rt.bignum {
        return fusevm::awk_host::awk_compl_checked(a.as_number())
            .map(Value::Num)
            .map_err(Error::Runtime);
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let f = value_to_mpfr(a, prec, round);
    if mpfr_negative(&f) {
        return Err(Error::Runtime(if f.is_integer() {
            format!(
                "compl({}): negative values are not allowed",
                float_trunc_integer(&f)
            )
        } else {
            format!("compl({}): negative value is not allowed", mpfr_fatal_g(&f))
        }));
    }
    let r = !float_trunc_integer(&f);
    Ok(Value::Mpfr(Float::with_val_round(prec, r, round).0))
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
    fn float_trunc_integer_truncates_toward_zero() {
        let f = Float::with_val(64, -9.7);
        let i = float_trunc_integer(&f);
        assert_eq!(format!("{i}"), "-9");
    }

    fn fold(op: BitFold, args: &[f64], rt: &Runtime) -> Result<Value> {
        let vals: Vec<Value> = args.iter().map(|&n| Value::Num(n)).collect();
        awk_bit_fold_values(op, &vals, rt)
    }

    #[test]
    fn awk_bit_values_without_bignum_are_gawk_64_bit() {
        let rt = Runtime::new();
        assert_eq!(
            fold(BitFold::And, &[12.0, 10.0], &rt).unwrap().as_number(),
            8.0
        );
        assert_eq!(
            fold(BitFold::Or, &[8.0, 1.0], &rt).unwrap().as_number(),
            9.0
        );
        assert_eq!(
            fold(BitFold::Xor, &[15.0, 3.0, 1.0], &rt)
                .unwrap()
                .as_number(),
            13.0
        );
        let sh = |l, a, n| awk_shift_values(l, &Value::Num(a), &Value::Num(n), &rt).unwrap();
        assert_eq!(sh(true, 1.0, 65.0).as_number(), 0.0);
        assert_eq!(sh(false, 16.0, 2.0).as_number(), 4.0);
        assert_eq!(
            awk_compl_values(&Value::Num(0.0), &rt).unwrap().as_number(),
            9007199254740991.0
        );
        assert!(awk_compl_values(&Value::Num(-1.0), &rt).is_err());
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
        assert_eq!(
            mpfr_dec(&fold(BitFold::And, &[12.0, 10.0], &rt).unwrap(), &rt),
            "8"
        );
        assert_eq!(
            mpfr_dec(&fold(BitFold::Or, &[12.0, 10.0], &rt).unwrap(), &rt),
            "14"
        );
        assert_eq!(
            mpfr_dec(&fold(BitFold::Xor, &[12.0, 10.0], &rt).unwrap(), &rt),
            "6"
        );
        let sh = |l, a, n| awk_shift_values(l, &Value::Num(a), &Value::Num(n), &rt).unwrap();
        assert_eq!(mpfr_dec(&sh(true, 3.0, 2.0), &rt), "12");
        assert_eq!(mpfr_dec(&sh(false, 17.0, 1.0), &rt), "8");
    }

    /// gawk 5.4.1 `-M` works on the arbitrary-precision integer: no 64-bit
    /// width, no `adjust_uint`, fractions truncated.
    #[test]
    fn awk_bitwise_bignum_is_unbounded_integer_math() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let two70 = 2f64.powi(70);
        assert_eq!(
            mpfr_dec(&fold(BitFold::Or, &[two70, 1.0], &rt).unwrap(), &rt),
            "1180591620717411303425"
        );
        // 2^70 + 4 is not a double; the operand string keeps it exact.
        let big = Value::Str("1180591620717411303428".into());
        let x = awk_bit_fold_values(BitFold::Xor, &[big, Value::Num(3.0), Value::Num(1.0)], &rt);
        assert_eq!(mpfr_dec(&x.unwrap(), &rt), "1180591620717411303430");
        let sh = |l, a, n| awk_shift_values(l, &Value::Num(a), &Value::Num(n), &rt).unwrap();
        assert_eq!(
            mpfr_dec(&sh(true, 1.0, 70.0), &rt),
            "1180591620717411303424"
        );
        assert_eq!(
            mpfr_dec(&sh(false, two70, 3.0), &rt),
            "147573952589676412928"
        );
        assert_eq!(mpfr_dec(&sh(true, 1.9, 2.9), &rt), "4");
    }

    /// gawk 5.4.1 `-M` fatals: `argument #N`, the right-most negative argument
    /// of a fold but the first of a shift, and `compl` wording that depends on
    /// whether the operand is integral.
    #[test]
    fn awk_bitwise_bignum_negative_fatals() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let err = |r: Result<Value>| r.unwrap_err().to_string();
        let e = err(fold(BitFold::And, &[-1.0, -2.0, 3.0], &rt));
        assert!(
            e.contains("and: argument #2 negative value -2 is not allowed"),
            "{e}"
        );
        let e = err(awk_shift_values(
            true,
            &Value::Num(-1.0),
            &Value::Num(-2.0),
            &rt,
        ));
        assert!(
            e.contains("lshift: argument #1 negative value -1 is not allowed"),
            "{e}"
        );
        let e = err(awk_shift_values(
            false,
            &Value::Num(1.0),
            &Value::Num(-2.5),
            &rt,
        ));
        assert!(
            e.contains("rshift: argument #2 negative value -2.5 is not allowed"),
            "{e}"
        );
        let e = err(awk_compl_values(&Value::Num(-3.0), &rt));
        assert!(
            e.contains("compl(-3): negative values are not allowed"),
            "{e}"
        );
        let e = err(awk_compl_values(&Value::Num(-0.5), &rt));
        assert!(
            e.contains("compl(-0.5): negative value is not allowed"),
            "{e}"
        );
    }

    /// Under `-M` gawk 5.4.1 complements the arbitrary-precision integer:
    /// `printf "%d", compl(0)` is `-1` and `compl(2^70)` is `-(2^70)-1`.
    #[test]
    fn awk_compl_bignum_is_integer_complement() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let v = awk_compl_values(&Value::Num(0.0), &rt).unwrap();
        assert_eq!(mpfr_dec(&v, &rt), "-1");
        let v = awk_compl_values(&Value::Num(2f64.powi(70)), &rt).unwrap();
        assert_eq!(mpfr_dec(&v, &rt), "-1180591620717411303425");
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
        let a = value_to_mpfr(&Value::Array(crate::runtime::AwkArray::new()), prec, round);
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
        assert_eq!(mpfr_string_trim_trailing_zeros("0.000".into()), "0");
        assert_eq!(
            mpfr_string_trim_trailing_zeros("123.45000".into()),
            "123.45"
        );
        assert_eq!(mpfr_string_trim_trailing_zeros("100".into()), "100");
    }

    #[test]
    fn value_to_mpfr_handles_non_numeric_strings_as_zero() {
        let prec = 64;
        let round = Round::Nearest;
        let v = value_to_mpfr(&Value::Str("not a number".into()), prec, round);
        assert!(v.is_zero());
    }

    #[test]
    fn awk_strtonum_value_decimal_integer_bignum_is_exact() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        // Large integer that would lose precision in f64
        let v = awk_strtonum_value("1267650600228229401496703205376", &rt);
        let s = mpfr_dec(&v, &rt);
        assert_eq!(s, "1267650600228229401496703205376");
    }

    #[test]
    fn awk_int_value_bignum_truncation() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let v = awk_int_value(&Value::Str("-123.456".into()), &rt);
        let s = mpfr_dec(&v, &rt);
        assert_eq!(s, "-123");
    }

    #[test]
    fn float_trunc_integer_large_value() {
        let mut rt = Runtime::new();
        rt.bignum = true;
        let i = Integer::from_str("100000000000000000000").unwrap();
        let f = Float::with_val(rt.mpfr_prec_bits(), i);
        let i2 = float_trunc_integer(&f);
        assert_eq!(format!("{i2}"), "100000000000000000000");
    }

    #[test]
    fn mpfr_to_f64_handles_subnormal() {
        let f = Float::with_val(64, 5e-324); // approx f64 subnormal min
        let n = f.to_f64();
        assert!(n > 0.0);
    }

    #[test]
    fn mpfr_from_f64_preserves_nan() {
        let f = Float::with_val(64, f64::NAN);
        assert!(f.is_nan());
    }

    #[test]
    fn mpfr_from_f64_preserves_inf() {
        let f = Float::with_val(64, f64::INFINITY);
        assert!(f.is_infinite() && f.is_sign_positive());
    }

    #[test]
    fn awk_and_bignum_v2() {
        let rt = Runtime::new();
        let a = Value::Num(255.0);
        let b = Value::Num(15.0);
        let res = super::awk_bit_fold_values(BitFold::And, &[a, b], &rt).unwrap();
        assert_eq!(res.as_number(), 15.0);
    }

    #[test]
    fn awk_or_bignum_v2() {
        let rt = Runtime::new();
        let a = Value::Num(240.0);
        let b = Value::Num(15.0);
        let res = super::awk_bit_fold_values(BitFold::Or, &[a, b], &rt).unwrap();
        assert_eq!(res.as_number(), 255.0);
    }

    #[test]
    fn awk_xor_bignum_v2() {
        let rt = Runtime::new();
        let a = Value::Num(255.0);
        let b = Value::Num(15.0);
        let res = super::awk_bit_fold_values(BitFold::Xor, &[a, b], &rt).unwrap();
        assert_eq!(res.as_number(), 240.0);
    }

    #[test]
    fn awk_compl_bignum_v2() {
        let rt = Runtime::new();
        // gawk: compl(-1) is a fatal, not a wrapped complement.
        assert!(super::awk_compl_values(&Value::Num(-1.0), &rt).is_err());
    }

    #[test]
    fn awk_lshift_bignum_v15() {
        let rt = Runtime::new();
        let res = super::awk_shift_values(true, &Value::Num(1.0), &Value::Num(10.0), &rt).unwrap();
        assert_eq!(res.as_number(), 1024.0);
    }

    #[test]
    fn awk_rshift_bignum_v15() {
        let rt = Runtime::new();
        let res =
            super::awk_shift_values(false, &Value::Num(1024.0), &Value::Num(10.0), &rt).unwrap();
        assert_eq!(res.as_number(), 1.0);
    }

    #[test]
    fn bignum_to_f64_precision_v15() {
        let big = Value::Str("1234567890123456789".into());
        let f = big.as_number();
        assert!(f > 1e18);
    }
}
