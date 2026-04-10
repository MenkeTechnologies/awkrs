//! MPFR / `-M` helpers: integer truncation, strtonum, intdiv, and string forms without f64 loss.

use crate::error::{Error, Result};
use crate::runtime::{value_to_float, Runtime, Value};
use rug::float::Round;
use rug::Float;
use rug::Integer;

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
    let f = value_to_float(v, prec);
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
    let fa = value_to_float(a, prec);
    let fb = value_to_float(b, prec);
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
    let t = s.trim();
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    if t.is_empty() {
        return Value::Mpfr(Float::with_val_round(prec, 0, round).0);
    }
    if t.starts_with("0x") || t.starts_with("0X") {
        return match Integer::from_str_radix(&t[2..], 16) {
            Ok(i) => Value::Mpfr(Float::with_val_round(prec, i, round).0),
            Err(_) => Value::Mpfr(Float::with_val_round(prec, 0, round).0),
        };
    }
    if t.len() > 1
        && t.starts_with('0')
        && !t.contains('.')
        && !t.contains('e')
        && !t.contains('E')
    {
        return match Integer::from_str_radix(t, 8) {
            Ok(i) => Value::Mpfr(Float::with_val_round(prec, i, round).0),
            Err(_) => Value::Mpfr(Float::with_val_round(prec, 0, round).0),
        };
    }
    match Float::parse(t) {
        Ok(ic) => Value::Mpfr(Float::with_val_round(prec, ic, round).0),
        Err(_) => Value::Mpfr(Float::with_val_round(prec, 0, round).0),
    }
}

pub fn awk_and_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_and(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_float(a, prec));
    let ub = float_trunc_u64(&value_to_float(b, prec));
    let r = ua & ub;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_or_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_or(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_float(a, prec));
    let ub = float_trunc_u64(&value_to_float(b, prec));
    let r = ua | ub;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_xor_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_xor(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_float(a, prec));
    let ub = float_trunc_u64(&value_to_float(b, prec));
    let r = ua ^ ub;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_lshift_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_lshift(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let x = float_trunc_u64(&value_to_float(a, prec));
    let n = float_trunc_u64(&value_to_float(b, prec)) & 0x3f;
    let r = x << n;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_rshift_values(a: &Value, b: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_rshift(a.as_number(), b.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let x = float_trunc_u64(&value_to_float(a, prec));
    let n = float_trunc_u64(&value_to_float(b, prec)) & 0x3f;
    let r = x >> n;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
}

pub fn awk_compl_values(a: &Value, rt: &Runtime) -> Value {
    if !rt.bignum {
        return Value::Num(crate::builtins::awk_compl(a.as_number()));
    }
    let prec = rt.mpfr_prec_bits();
    let round = rt.mpfr_round();
    let ua = float_trunc_u64(&value_to_float(a, prec));
    let r = !ua;
    Value::Mpfr(Float::with_val_round(prec, Integer::from(r), round).0)
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
}
