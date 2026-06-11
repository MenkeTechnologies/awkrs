//! `%g` rounding-carry edge cases for `format_g_decimal_significant_f64`
//! (`src/format.rs:699`).
//!
//! `%g` precision is *significant digits*, and the fixed-vs-scientific choice
//! plus the digit-count both depend on the value's decimal exponent. The
//! implementation computes that exponent twice on purpose:
//!
//! ```ignore
//! let e = n.log10().floor() as i32;                 // pre-round magnitude
//! let r = (n * sig_scale).round() / sig_scale;      // round to p sig-digits
//! let e2 = r.log10().floor() as i32;                // post-round magnitude
//! let frac = (p as i32 - e2 - 1).max(0) as usize;   // fraction width
//! ```
//!
//! The `e2` recompute exists solely so that a value whose rounding *carries*
//! into a new power of ten (e.g. `9.96` → `10` at 2 sig-digits) reports the
//! correct fraction width and, at higher magnitudes, flips into `%e` style at
//! the right boundary. If anyone deletes the recompute and reuses `e`, the
//! fraction width is computed off the pre-round exponent and these outputs
//! drift by a digit (`10` would render as `10.` / `1.0e+01`, `0.1` as `0.10`).
//!
//! Not boilerplate: every existing `%g` test in `src/format.rs`
//! (`percent_g_precision_one_emits_single_significant_digit`,
//! `..._two_keeps_two_significant_digits`, `percent_g_uses_significant_digits_*`)
//! formats `123.456`, whose mantissa `1.23` never rounds across a power-of-ten
//! boundary, so the `e != e2` carry branch is entirely unexercised. These
//! assert the gawk/POSIX reference for the carry cases specifically, pinning
//! both the fixed-form carry (`9.96`/`0.0996`) and the carry that pushes a
//! value past the fixed→scientific threshold (`9.96`/`%.1g`, `999999.5`/`%.6g`).

mod common;

use common::run_awkrs_stdin;

/// `%.2g` of `9.96`: rounding 9.96 to 2 significant digits yields 10.0, which
/// has magnitude 10^1 — one higher than the pre-round mantissa `9.96` (10^0).
/// The post-round exponent must drive the fraction width to 0 so the result is
/// `10`, not `10.` or `9.96`. gawk, the One True awk, and mawk all print `10`.
#[test]
fn percent_g_two_sig_digits_carries_into_new_power_of_ten() {
    let (code, out, _) = run_awkrs_stdin(r#"BEGIN { printf "[%.2g]\n", 9.96 }"#, "");
    assert_eq!(code, 0);
    assert_eq!(out, "[10]\n", "got {out:?}");
}

/// `%.2g` of `0.0996`: same carry, but in the sub-1 range. 0.0996 rounds to
/// 0.10 at 2 sig-digits; the trailing significant zero is dropped by `%g`,
/// leaving `0.1`. The post-round exponent (10^-1) differs from the pre-round
/// exponent (10^-2), so a stale `e` would mis-size the fraction width.
#[test]
fn percent_g_two_sig_digits_carries_in_fractional_range() {
    let (code, out, _) = run_awkrs_stdin(r#"BEGIN { printf "[%.2g]\n", 0.0996 }"#, "");
    assert_eq!(code, 0);
    assert_eq!(out, "[0.1]\n", "got {out:?}");
}

/// `%.1g` of `9.96`: rounding to 1 significant digit gives 10, whose exponent
/// (1) hits the `%g` rule "use %e when exp >= precision". At precision 1 the
/// post-round value crosses that threshold *because of the carry* — the
/// pre-round 9.96 (exp 0) would not. Result is scientific `1e+01`, matching
/// gawk. This pins the carry interaction with the fixed→scientific switch.
#[test]
fn percent_g_one_sig_digit_carry_flips_to_scientific() {
    let (code, out, _) = run_awkrs_stdin(r#"BEGIN { printf "[%.1g]\n", 9.96 }"#, "");
    assert_eq!(code, 0);
    assert_eq!(out, "[1e+01]\n", "got {out:?}");
}

/// `%.6g` of `999999.5`: rounding to 6 significant digits carries 999999.5 up
/// to 1000000 (10^6). With precision 6, an exponent of 6 triggers scientific
/// form, so gawk prints `1e+06`. Without the `e2` recompute the pre-round
/// exponent (5) keeps it in fixed form and the value renders as `1.00000e+06`
/// or `1000000`. This is the large-magnitude analogue of the `%.1g` carry.
#[test]
fn percent_g_six_sig_digits_carry_at_million_boundary() {
    let (code, out, _) = run_awkrs_stdin(r#"BEGIN { printf "[%.6g]\n", 999999.5 }"#, "");
    assert_eq!(code, 0);
    assert_eq!(out, "[1e+06]\n", "got {out:?}");
}
