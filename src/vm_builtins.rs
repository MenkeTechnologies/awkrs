// Builtin-call dispatch for the awkrs VM, split out of vm.rs to keep that file manageable.
// Child module of `vm` via `#[path]`; `use super::*` resolves to vm.rs's items.

use super::*;

pub(super) fn exec_call_builtin(ctx: &mut VmCtx<'_>, name: &str, argc: u16) -> Result<()> {
    let argc = argc as usize;

    // First, check if it's a user function
    if ctx.cp.functions.contains_key(name) {
        return exec_call_user(ctx, name, argc as u16);
    }

    let start = ctx.stack.len() - argc;
    let args: Vec<Value> = ctx.stack.drain(start..).collect();

    let result = exec_builtin_dispatch(ctx, name, args)?;
    ctx.push(result);
    Ok(())
}

/// Core builtin implementation (also used by JIT `MIXED_BUILTIN_CALL`).
pub(crate) fn exec_builtin_dispatch(
    ctx: &mut VmCtx<'_>,
    name: &str,
    args: Vec<Value>,
) -> Result<Value> {
    // In POSIX / traditional mode, reject gawk-only extension functions
    if ctx.rt.posix || ctx.rt.traditional {
        const GAWK_ONLY_BUILTINS: &[&str] = &[
            "and",
            "or",
            "xor",
            "compl",
            "lshift",
            "rshift",
            "gensub",
            "patsplit",
            "mkbool",
            "mktime",
            "strftime",
            "systime",
            "isarray",
            "typeof",
            "strtonum",
            "dcgettext",
            "dcngettext",
            "bindtextdomain",
            "chdir",
            "stat",
            "statvfs",
            "fts",
            "chr",
            "ord",
            "gettimeofday",
            "getlocaltime",
            "sleep",
            "readfile",
            "readdir",
            "reada",
            "writea",
            "inplace_tmpfile",
            "inplace_commit",
            "rename",
            "revoutput",
            "revtwoway",
            "intdiv",
            "intdiv0",
        ];
        if GAWK_ONLY_BUILTINS.contains(&name) {
            return Err(Error::Runtime(format!(
                "`{name}` is a gawk extension not available in POSIX/traditional mode"
            )));
        }
    }
    let argc = args.len();
    let result = match name {
        "length" => {
            // gawk parity: length takes 0 or 1 argument. `length("a", "b")`
            // fatals with "2 is invalid as number of arguments for length".
            // Previously awkrs silently ignored extra args.
            if argc > 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for length"
                )));
            }
            if args.is_empty() {
                let n = if ctx.rt.characters_as_bytes {
                    ctx.rt.record.len()
                } else {
                    ctx.rt.record.chars().count()
                };
                Value::Num(n as f64)
            } else {
                match &args[0] {
                    Value::Array(a) => Value::Num(a.len() as f64),
                    v => {
                        let s = v.as_str();
                        let n = if ctx.rt.characters_as_bytes {
                            s.len()
                        } else {
                            s.chars().count()
                        };
                        Value::Num(n as f64)
                    }
                }
            }
        }
        "index" => {
            if argc != 2 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for index"
                )));
            }
            let hay = args[0].as_str();
            let needle = args[1].as_str();
            if needle.is_empty() {
                Value::Num(1.0)
            } else {
                // gawk parity: `IGNORECASE` applies to `index()` as well,
                // so `IGNORECASE=1; index("ABC", "b")` returns 2. The
                // case-insensitive search is done via lowercased copies
                // (cheap for short needles, which is the common case).
                let pos = if ctx.rt.ignore_case_flag() {
                    let hay_lc = hay.to_lowercase();
                    let needle_lc = needle.to_lowercase();
                    hay_lc.find(needle_lc.as_str()).map(|b| {
                        if ctx.rt.characters_as_bytes {
                            b + 1
                        } else {
                            // The byte offset in `hay_lc` matches the byte
                            // offset in `hay` for ASCII; for non-ASCII this
                            // can differ in length but the relative position
                            // is preserved for common cases. Iterate `hay`'s
                            // chars to convert byte to char position.
                            hay.char_indices().take_while(|&(off, _)| off < b).count() + 1
                        }
                    })
                } else {
                    hay.find(needle.as_str()).map(|b| {
                        if ctx.rt.characters_as_bytes {
                            b + 1
                        } else {
                            hay[..b].chars().count() + 1
                        }
                    })
                };
                match pos {
                    Some(p) => Value::Num(p as f64),
                    None => Value::Num(0.0),
                }
            }
        }
        "substr" => {
            if !(2..=3).contains(&argc) {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for substr"
                )));
            }
            let s = args[0].as_str();
            let start_raw = args[1].as_number();
            let mut m = start_raw as i64;
            let len_opt = if let Some(v) = args.get(2) {
                let l = v.as_number() as i64;
                if l <= 0 {
                    return Ok(Value::Str(String::new()));
                }
                Some(l)
            } else {
                None
            };
            // gawk: start < 1 is treated as 1; length is not shortened (POSIX extension).
            if m < 1 {
                ctx.rt.lint_warn(&format!(
                    "substr: start index {start_raw} is less than 1, treated as 1"
                ));
                m = 1;
            }
            let len = len_opt.map(|l| l as usize).unwrap_or(usize::MAX);
            let start0 = (m as usize).saturating_sub(1);
            if ctx.rt.characters_as_bytes {
                let b = s.as_bytes();
                let slice = b
                    .get(start0..)
                    .map(|rest| {
                        let take = len.min(rest.len());
                        String::from_utf8_lossy(&rest[..take]).into_owned()
                    })
                    .unwrap_or_default();
                Value::Str(slice)
            } else {
                let slice: String = s.chars().skip(start0).take(len).collect();
                Value::Str(slice)
            }
        }
        "tolower" => {
            // gawk parity: `tolower()` with no args is a runtime error, not a
            // panic. Earlier awkrs indexed args[0] unchecked.
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for tolower"
                )));
            }
            Value::Str(args[0].as_str().to_lowercase())
        }
        "toupper" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for toupper"
                )));
            }
            Value::Str(args[0].as_str().to_uppercase())
        }
        "int" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for int"
                )));
            }
            bignum::awk_int_value(&args[0], ctx.rt)
        }
        "intdiv" => {
            if argc != 2 {
                return Err(Error::Runtime("`intdiv` expects two arguments".into()));
            }
            bignum::awk_intdiv_values(&args[0], &args[1], ctx.rt)?
        }
        "mkbool" => {
            if argc != 1 {
                return Err(Error::Runtime("`mkbool` expects one argument".into()));
            }
            Value::Num(if truthy(&args[0])? { 1.0 } else { 0.0 })
        }
        "sqrt" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for sqrt"
                )));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                if matches!(f.cmp0(), Some(Ordering::Less)) {
                    ctx.rt.warn_builtin_negative_arg("sqrt", f.to_f64());
                }
                Value::Mpfr(Float::with_val_round(prec, f.sqrt(), round).0)
            } else {
                let x = args[0].as_number();
                if x < 0.0 {
                    ctx.rt.warn_builtin_negative_arg("sqrt", x);
                }
                // Normalize NaN sign — Linux glibc sqrt(-1) sets the sign bit
                // (would print as `-nan`); gawk emits `+nan` regardless of platform.
                let r = x.sqrt();
                Value::Num(if r.is_nan() { f64::NAN } else { r })
            }
        }
        "sin" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for sin"
                )));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                Value::Mpfr(Float::with_val_round(prec, f.sin(), round).0)
            } else {
                // Normalize NaN sign — Linux glibc sin(±inf) may set the sign bit
                // (would print as `-nan`); gawk emits `+nan` regardless of platform.
                let r = args[0].as_number().sin();
                Value::Num(if r.is_nan() { f64::NAN } else { r })
            }
        }
        "cos" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for cos"
                )));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                Value::Mpfr(Float::with_val_round(prec, f.cos(), round).0)
            } else {
                // Normalize NaN sign — same Linux glibc cos(±inf) edge case.
                let r = args[0].as_number().cos();
                Value::Num(if r.is_nan() { f64::NAN } else { r })
            }
        }
        "atan2" => {
            if argc != 2 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for atan2"
                )));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let y = value_to_float(&args[0], prec, round);
                let x = value_to_float(&args[1], prec, round);
                Value::Mpfr(Float::with_val_round(prec, y.atan2(&x), round).0)
            } else {
                // Normalize NaN sign — atan2 with NaN input propagates the sign bit.
                let r = args[0].as_number().atan2(args[1].as_number());
                Value::Num(if r.is_nan() { f64::NAN } else { r })
            }
        }
        "exp" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for exp"
                )));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                Value::Mpfr(Float::with_val_round(prec, f.exp(), round).0)
            } else {
                // exp(-NaN) propagates the sign bit; normalize for gawk parity.
                let r = args[0].as_number().exp();
                Value::Num(if r.is_nan() { f64::NAN } else { r })
            }
        }
        "log" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for log"
                )));
            }
            if ctx.rt.bignum {
                let prec = ctx.rt.mpfr_prec_bits();
                let round = ctx.rt.mpfr_round();
                let f = value_to_float(&args[0], prec, round);
                match f.cmp0() {
                    Some(Ordering::Less) => {
                        ctx.rt.warn_builtin_negative_arg("log", f.to_f64());
                    }
                    Some(Ordering::Equal) => {
                        ctx.rt.lint_warn("log: zero argument yields -infinity");
                    }
                    Some(Ordering::Greater) | None => {}
                }
                Value::Mpfr(Float::with_val_round(prec, f.ln(), round).0)
            } else {
                let x = args[0].as_number();
                if x < 0.0 {
                    ctx.rt.warn_builtin_negative_arg("log", x);
                } else if x == 0.0 {
                    ctx.rt.lint_warn("log: zero argument yields -infinity");
                }
                // Normalize NaN sign — Linux glibc log(-1) sets the sign bit
                // (would print as `-nan`); gawk emits `+nan` regardless of platform.
                let r = x.ln();
                Value::Num(if r.is_nan() { f64::NAN } else { r })
            }
        }
        "systime" => {
            if argc != 0 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for systime"
                )));
            }
            Value::Num(builtins::awk_systime())
        }
        "strftime" => builtins::awk_strftime(&args).map_err(Error::Runtime)?,
        "mktime" => {
            // gawk parity: `mktime(datespec [, utc])` — when the optional second
            // argument is truthy, interpret the datespec in UTC, otherwise in
            // local time.
            if !(1..=2).contains(&argc) {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for mktime"
                )));
            }
            let utc = argc == 2 && args[1].as_number() != 0.0;
            Value::Num(builtins::awk_mktime_with_utc(&args[0].as_str(), utc))
        }
        "rand" => {
            // gawk parity: rand takes zero arguments.
            if argc != 0 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for rand"
                )));
            }
            Value::Num(ctx.rt.rand())
        }
        "srand" => {
            // gawk parity: srand takes zero or one argument.
            if argc > 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for srand"
                )));
            }
            let n = match args.first() {
                None => None,
                Some(v) => {
                    if ctx.rt.bignum {
                        let prec = ctx.rt.mpfr_prec_bits();
                        let round = ctx.rt.mpfr_round();
                        let f = value_to_float(v, prec, round);
                        Some(bignum::float_trunc_integer(&f).to_u64_wrapping())
                    } else {
                        Some(v.as_number() as u32 as u64)
                    }
                }
            };
            Value::Num(ctx.rt.srand(n))
        }
        "system" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for system"
                )));
            }
            if ctx.rt.sandbox {
                return Err(Error::Runtime(
                    "sandbox: system() is disabled (-S/--sandbox)".into(),
                ));
            }
            use std::process::Command;
            // POSIX/gawk: flush stdout and any buffered pipes/files before launching
            // the subprocess so its output is correctly interleaved after pending awk
            // output rather than before it. Without this, `print "a"; system("echo b")`
            // would emit "b" before the buffered "a".
            flush_print_buf(&mut ctx.rt.print_buf)?;
            let _ = std::io::stdout().flush();
            ctx.rt.flush_all_output_handles();
            let cmd = args[0].as_str();
            let st = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .status()
                .map_err(Error::Io)?;
            Value::Num(st.code().unwrap_or(-1) as f64)
        }
        "close" => {
            // gawk: `close(cmd)` closes the stream; `close(cmd, "to"|"from")`
            // closes one direction of a coprocess. awkrs doesn't (yet) implement
            // bidirectional coprocesses with directional close — accept the 2-arg
            // form and treat it as a plain close so user scripts don't error.
            if argc != 1 && argc != 2 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for close"
                )));
            }
            let path = args[0].as_str();
            Value::Num(ctx.rt.close_handle(&path))
        }
        "fflush" => {
            if args.is_empty() {
                ctx.emit_flush()?;
            } else {
                let path = args[0].as_str();
                if path.is_empty() {
                    ctx.emit_flush()?;
                } else {
                    ctx.rt.flush_redirect_target(&path)?;
                }
            }
            Value::Num(0.0)
        }
        "sprintf" => {
            if args.is_empty() {
                return Err(Error::Runtime("sprintf: need format".into()));
            }
            let fmt = args[0].as_str();
            sprintf_simple(
                &fmt,
                &args[1..],
                ctx.rt.numeric_decimal,
                ctx.rt.numeric_thousands_sep,
                ctx.rt,
            )?
        }
        "printf" => {
            if args.is_empty() {
                return Err(Error::Runtime("printf: need format".into()));
            }
            let fmt = args[0].as_str();
            let s = sprintf_simple(
                &fmt,
                &args[1..],
                ctx.rt.numeric_decimal,
                ctx.rt.numeric_thousands_sep,
                ctx.rt,
            )?
            .as_str();
            ctx.emit_print(&s);
            Value::Num(0.0)
        }
        "and" => {
            if argc < 2 {
                return Err(Error::Runtime(
                    "and: called with less than two arguments".into(),
                ));
            }
            let mut acc = bignum::awk_and_values(&args[0], &args[1], ctx.rt);
            for a in &args[2..] {
                acc = bignum::awk_and_values(&acc, a, ctx.rt);
            }
            acc
        }
        "or" => {
            if argc < 2 {
                return Err(Error::Runtime(
                    "or: called with less than two arguments".into(),
                ));
            }
            let mut acc = bignum::awk_or_values(&args[0], &args[1], ctx.rt);
            for a in &args[2..] {
                acc = bignum::awk_or_values(&acc, a, ctx.rt);
            }
            acc
        }
        "xor" => {
            if argc < 2 {
                return Err(Error::Runtime(
                    "xor: called with less than two arguments".into(),
                ));
            }
            let mut acc = bignum::awk_xor_values(&args[0], &args[1], ctx.rt);
            for a in &args[2..] {
                acc = bignum::awk_xor_values(&acc, a, ctx.rt);
            }
            acc
        }
        "lshift" => {
            if argc != 2 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for lshift"
                )));
            }
            // gawk parity: negative shift count is a fatal runtime error.
            let av = args[0].as_number();
            let bv = args[1].as_number();
            if av < 0.0 {
                return Err(Error::Runtime(format!(
                    "lshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            if bv < 0.0 {
                return Err(Error::Runtime(format!(
                    "lshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            bignum::awk_lshift_values(&args[0], &args[1], ctx.rt)
        }
        "rshift" => {
            if argc != 2 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for rshift"
                )));
            }
            let av = args[0].as_number();
            let bv = args[1].as_number();
            if av < 0.0 {
                return Err(Error::Runtime(format!(
                    "rshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            if bv < 0.0 {
                return Err(Error::Runtime(format!(
                    "rshift({av:.6}, {bv:.6}): negative values are not allowed"
                )));
            }
            bignum::awk_rshift_values(&args[0], &args[1], ctx.rt)
        }
        "compl" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for compl"
                )));
            }
            let av = args[0].as_number();
            if av < 0.0 {
                return Err(Error::Runtime(format!(
                    "compl({av:.6}): negative value is not allowed"
                )));
            }
            bignum::awk_compl_values(&args[0], ctx.rt)
        }
        "strtonum" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for strtonum"
                )));
            }
            bignum::awk_strtonum_value(&args[0].as_str(), ctx.rt)
        }
        "typeof" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for typeof"
                )));
            }
            Value::Str(builtins::awk_typeof_value(&args[0]).into())
        }
        "gensub" => {
            if !(3..=4).contains(&argc) {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for gensub"
                )));
            }
            let target = if argc == 4 {
                Some(args[3].as_str())
            } else {
                None
            };
            let out = builtins::awk_gensub(
                ctx.rt,
                &args[0].as_str(),
                &args[1].as_str(),
                &args[2],
                target,
            )?;
            Value::Str(out)
        }
        "isarray" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for isarray"
                )));
            }
            Value::Num(match &args[0] {
                Value::Array(_) => 1.0,
                _ => 0.0,
            })
        }
        "bindtextdomain" => {
            if argc != 2 {
                return Err(Error::Runtime(
                    "`bindtextdomain` expects two arguments (domain, dirname)".into(),
                ));
            }
            let domain = args[0].as_str();
            let dirname = args[1].as_str();
            ctx.rt.gettext_dir = dirname.clone();
            ctx.rt
                .vars
                .insert("TEXTDOMAIN".into(), Value::Str(domain.clone()));
            if let Some(cat) = crate::gettext_util::try_load_gettext_catalog(&domain, &dirname) {
                ctx.rt.gettext_catalogs.insert(domain, cat);
            }
            Value::Str(dirname)
        }
        "dcgettext" => {
            if argc != 3 {
                return Err(Error::Runtime(
                    "`dcgettext` expects three arguments (string, domain, category)".into(),
                ));
            }
            let msgid = args[0].as_str();
            let domain = args[1].as_str();
            let _cat = args[2].as_number() as i32;
            if let Some(cat) = ctx.rt.gettext_catalogs.get(&domain) {
                Value::Str(cat.gettext(msgid.as_str()).to_string())
            } else {
                Value::Str(msgid)
            }
        }
        "dcngettext" => {
            if argc != 5 {
                return Err(Error::Runtime(
                    "`dcngettext` expects five arguments (s1, s2, n, domain, category)".into(),
                ));
            }
            let s1 = args[0].as_str();
            let s2 = args[1].as_str();
            let n = args[2].as_number();
            let domain = args[3].as_str();
            let _ = args[4].as_number() as i32;
            if let Some(cat) = ctx.rt.gettext_catalogs.get(&domain) {
                Value::Str(cat.ngettext(s1.as_str(), s2.as_str(), n as u64).to_string())
            } else {
                Value::Str((if n == 1.0 { s1 } else { s2 }).to_string())
            }
        }
        "chdir" => {
            if argc != 1 {
                return Err(Error::Runtime("`chdir` expects one argument".into()));
            }
            crate::gawk_extensions::chdir(ctx.rt, &args[0].as_str())?
        }
        "stat" => {
            if argc != 2 {
                return Err(Error::Runtime("`stat` expects two arguments".into()));
            }
            crate::gawk_extensions::stat(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "statvfs" => {
            if argc != 2 {
                return Err(Error::Runtime("`statvfs` expects two arguments".into()));
            }
            crate::gawk_extensions::statvfs(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "fts" => {
            if argc != 2 {
                return Err(Error::Runtime("`fts` expects two arguments".into()));
            }
            crate::gawk_extensions::fts(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "gettimeofday" => {
            if argc != 1 {
                return Err(Error::Runtime("`gettimeofday` expects one argument".into()));
            }
            crate::gawk_extensions::gettimeofday(ctx.rt, &args[0].as_str())?
        }
        "sleep" => {
            if argc != 1 {
                return Err(Error::Runtime("`sleep` expects one argument".into()));
            }
            crate::gawk_extensions::sleep_secs(ctx.rt, args[0].as_number())?
        }
        "ord" => {
            if argc != 1 {
                return Err(Error::Runtime("`ord` expects one argument".into()));
            }
            crate::gawk_extensions::ord(ctx.rt, &args[0].as_str())?
        }
        "chr" => {
            if argc != 1 {
                return Err(Error::Runtime("`chr` expects one argument".into()));
            }
            crate::gawk_extensions::chr(ctx.rt, args[0].as_number())?
        }
        "readfile" => {
            if argc != 1 {
                return Err(Error::Runtime("`readfile` expects one argument".into()));
            }
            crate::gawk_extensions::readfile(ctx.rt, &args[0].as_str())?
        }
        "revoutput" => {
            if argc != 1 {
                return Err(Error::Runtime("`revoutput` expects one argument".into()));
            }
            crate::gawk_extensions::revoutput(ctx.rt, &args[0].as_str())?
        }
        "revtwoway" => {
            if argc != 1 {
                return Err(Error::Runtime("`revtwoway` expects one argument".into()));
            }
            crate::gawk_extensions::revtwoway(ctx.rt, &args[0].as_str())?
        }
        "rename" => {
            if argc != 2 {
                return Err(Error::Runtime("`rename` expects two arguments".into()));
            }
            crate::gawk_extensions::rename(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "inplace_tmpfile" => {
            if argc != 1 {
                return Err(Error::Runtime(
                    "`inplace_tmpfile` expects one argument".into(),
                ));
            }
            crate::gawk_extensions::inplace_tmpfile(ctx.rt, &args[0].as_str())?
        }
        "inplace_commit" => {
            if argc != 2 {
                return Err(Error::Runtime(
                    "`inplace_commit` expects two arguments".into(),
                ));
            }
            crate::gawk_extensions::inplace_commit(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "writea" => {
            if argc != 2 {
                return Err(Error::Runtime("`writea` expects two arguments".into()));
            }
            crate::gawk_extensions::writea(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "reada" => {
            if argc != 2 {
                return Err(Error::Runtime("`reada` expects two arguments".into()));
            }
            crate::gawk_extensions::reada(ctx.rt, &args[0].as_str(), &args[1].as_str())?
        }
        "intdiv0" => {
            if argc != 2 {
                return Err(Error::Runtime("`intdiv0` expects two arguments".into()));
            }
            crate::gawk_extensions::intdiv0(ctx.rt, &args[0], &args[1])?
        }
        "readdir" => {
            if argc != 2 {
                return Err(Error::Runtime(
                    "`readdir` expects two arguments (path, array)".into(),
                ));
            }
            let path = args[0].as_str().to_string();
            let arr_name = args[1].as_str().to_string();
            crate::gawk_extensions::readdir(ctx.rt, &path, &arr_name)?
        }
        "getlocaltime" => {
            if !(1..=2).contains(&argc) {
                return Err(Error::Runtime(
                    "`getlocaltime` expects 1 or 2 arguments (array [, timestamp])".into(),
                ));
            }
            let arr_name = args[0].as_str().to_string();
            let ts = if argc == 2 {
                Some(args[1].as_number())
            } else {
                None
            };
            crate::gawk_extensions::getlocaltime(ctx.rt, &arr_name, ts)?
        }
        // ── AOP function-call intercepts (awkrs/zshrs-original extension) ──────
        "intercept" => {
            // intercept("before"|"after"|"around", pattern, code) — register
            // advice; returns the new intercept ID.
            if argc != 3 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for intercept (want kind, pattern, code)"
                )));
            }
            let kind_s = args[0].as_str();
            let kind = match kind_s.as_str() {
                "before" => crate::intercepts::AdviceKind::Before,
                "after" => crate::intercepts::AdviceKind::After,
                "around" => crate::intercepts::AdviceKind::Around,
                other => {
                    return Err(Error::Runtime(format!(
                        "intercept: unknown advice kind `{other}` (want before|after|around)"
                    )))
                }
            };
            let pattern = args[1].as_str();
            let code = args[2].as_str();
            // Compile the advice against the running program so it shares live
            // globals/arrays and can call the original via intercept_proceed().
            let program = crate::compiler::Compiler::compile_advice(&code, ctx.cp)?;
            let id = ctx.rt.intercepts.iter().map(|i| i.id).max().unwrap_or(0) + 1;
            ctx.rt.intercepts.push(crate::intercepts::Intercept {
                pattern,
                kind,
                code,
                id,
                program: std::sync::Arc::new(program),
            });
            Value::Num(id as f64)
        }
        "intercept_list" => {
            // Diagnostic listing goes to stderr (awk stdout is the data stream);
            // the return value is the count of registered intercepts.
            if !ctx.rt.intercepts.is_empty() {
                eprintln!("{:>4}  {:<8}  {:<20}  CODE", "ID", "KIND", "PATTERN");
                for i in &ctx.rt.intercepts {
                    let kind = match i.kind {
                        crate::intercepts::AdviceKind::Before => "before",
                        crate::intercepts::AdviceKind::After => "after",
                        crate::intercepts::AdviceKind::Around => "around",
                    };
                    let code_preview: String = if i.code.chars().count() > 40 {
                        let mut s: String = i.code.chars().take(37).collect();
                        s.push_str("...");
                        s
                    } else {
                        i.code.clone()
                    };
                    eprintln!(
                        "{:>4}  {:<8}  {:<20}  {}",
                        i.id, kind, i.pattern, code_preview
                    );
                }
            }
            Value::Num(ctx.rt.intercepts.len() as f64)
        }
        "intercept_remove" => {
            if argc != 1 {
                return Err(Error::Runtime(format!(
                    "{argc} is invalid as number of arguments for intercept_remove (want id)"
                )));
            }
            let id = args[0].as_number() as u32;
            let before = ctx.rt.intercepts.len();
            ctx.rt.intercepts.retain(|i| i.id != id);
            Value::Num(if ctx.rt.intercepts.len() < before {
                1.0
            } else {
                0.0
            })
        }
        "intercept_clear" => {
            let n = ctx.rt.intercepts.len();
            ctx.rt.intercepts.clear();
            Value::Num(n as f64)
        }
        "intercept_proceed" => {
            // Called from around advice: run the original function with its real
            // arguments and return its value. Bypasses the intercept check
            // (`exec_call_user_inner` does not re-fire advice), so an around
            // advice that unconditionally proceeds won't recurse on itself.
            let (fname, fargs) = match ctx.rt.intercept_call_stack.last() {
                Some(c) => (c.name.clone(), c.args.clone()),
                None => {
                    return Err(Error::Runtime(
                        "intercept_proceed: called outside around advice".into(),
                    ))
                }
            };
            ctx.rt
                .vars
                .insert("__intercept_proceed".into(), Value::Str("1".into()));
            let v = exec_call_user_inner(ctx, &fname, fargs)?;
            if let Some(c) = ctx.rt.intercept_call_stack.last_mut() {
                c.proceeded = true;
                c.result = v.clone();
            }
            v
        }
        // Inline Rust FFI: the `rust { ... }` desugar emits a `BEGIN` rule that
        // calls `__rust_compile(b64, line)`; compile + register the block's
        // exported functions.
        "__rust_compile" => {
            let b64 = args.first().map(|v| v.as_str()).unwrap_or_default();
            fusevm::ffi::compile_and_register(&b64).map_err(Error::Runtime)?;
            Value::Uninit
        }
        // A `rust { ... }` block's exported functions are callable by bareword.
        // User awk functions win (`exec_call_builtin` resolves them before this
        // dispatch), and every language builtin matches an earlier arm; the FFI
        // registry is the last resort before "unknown function", and the
        // membership check keeps it off the hot path.
        _ => {
            if fusevm::ffi::is_registered(name) {
                let fargs: Vec<fusevm::Value> = args.iter().map(awk_value_to_fusevm).collect();
                if let Some(r) = fusevm::ffi::try_call(name, &fargs) {
                    return r.map(fusevm_value_to_awk).map_err(Error::Runtime);
                }
            }
            return Err(Error::Runtime(format!("unknown function `{name}`")));
        }
    };
    Ok(result)
}

/// Marshal an awk [`Value`] into a [`fusevm::Value`] for an FFI call. Numbers
/// map to `Float` (fusevm coerces to `i64`/`f64` per the target signature);
/// everything else marshals via its string form, which fusevm parses when the
/// target parameter is numeric (so numeric-string fields work as int/float args).
fn awk_value_to_fusevm(v: &Value) -> fusevm::Value {
    match v {
        Value::Num(n) => fusevm::Value::float(*n),
        Value::Mpfr(f) => fusevm::Value::float(f.to_f64()),
        _ => fusevm::Value::str(v.as_str()),
    }
}

/// Marshal an FFI [`fusevm::Value`] result back into an awk [`Value`].
fn fusevm_value_to_awk(v: fusevm::Value) -> Value {
    match v {
        fusevm::Value::Int(n) => Value::Num(n as f64),
        fusevm::Value::Float(f) => Value::Num(f),
        fusevm::Value::Undef => Value::Uninit,
        other => Value::Str(other.to_str()),
    }
}

pub(super) fn sort_keys_with_custom_cmp(
    ctx: &mut VmCtx<'_>,
    keys: &mut [String],
    fname: &str,
    arr_name: &str,
) -> Result<()> {
    let Some(func) = ctx.cp.functions.get(fname) else {
        return Err(Error::Runtime(format!(
            "sorted_in: unknown function `{fname}`"
        )));
    };
    let argc = func.params.len();
    if !(argc == 2 || argc == 4) {
        return Err(Error::Runtime(format!(
            "sorted_in: comparison function `{fname}` must have 2 or 4 parameters (has {argc})"
        )));
    }

    let err: RefCell<Option<Error>> = RefCell::new(None);
    keys.sort_by(|a, b| {
        if err.borrow().is_some() {
            return Ordering::Equal;
        }
        let vals = if argc == 2 {
            vec![Value::Str(a.clone()), Value::Str(b.clone())]
        } else {
            let va = if arr_name == "SYMTAB" {
                ctx.rt.symtab_elem_get(a.as_str())
            } else {
                ctx.rt.array_get(arr_name, a.as_str())
            };
            let vb = if arr_name == "SYMTAB" {
                ctx.rt.symtab_elem_get(b.as_str())
            } else {
                ctx.rt.array_get(arr_name, b.as_str())
            };
            vec![Value::Str(a.clone()), va, Value::Str(b.clone()), vb]
        };
        match exec_call_user_inner(ctx, fname, vals) {
            Ok(v) => {
                let n = v.as_number();
                if n < 0.0 {
                    Ordering::Less
                } else if n > 0.0 {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            }
            Err(e) => {
                *err.borrow_mut() = Some(e);
                Ordering::Equal
            }
        }
    });
    if let Some(e) = err.into_inner() {
        return Err(e);
    }
    Ok(())
}

/// Run a user function with explicit arguments (VM stack path and JIT `MIXED_CALL_USER_*`).
pub(crate) fn exec_call_user_inner(
    ctx: &mut VmCtx<'_>,
    name: &str,
    mut vals: Vec<Value>,
) -> Result<Value> {
    let func = ctx
        .cp
        .functions
        .get(name)
        .ok_or_else(|| Error::Runtime(format!("unknown function `{name}`")))?
        .clone();

    if ctx.locals.len() >= crate::limits::MAX_USER_CALL_DEPTH {
        return Err(Error::Runtime(format!(
            "maximum user function call depth ({}) exceeded",
            crate::limits::MAX_USER_CALL_DEPTH
        )));
    }

    while vals.len() < func.params.len() {
        vals.push(Value::Uninit);
    }
    vals.truncate(func.params.len());

    let mut frame = AwkMap::default();
    for (p, v) in func.params.iter().zip(vals) {
        frame.insert(p.clone(), v);
    }
    crate::vm::debugger_enter_sub(ctx, name);
    ctx.locals.push(frame);
    let was_fn = ctx.in_function;
    ctx.in_function = true;

    let result = match execute(&func.body, ctx) {
        Ok(VmSignal::Normal) => Value::Uninit,
        Ok(VmSignal::Return(v)) => v,
        Ok(VmSignal::Next) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            crate::vm::debugger_leave_sub(ctx);
            return Err(Error::Runtime("invalid jump out of function (next)".into()));
        }
        Ok(VmSignal::NextFile) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            crate::vm::debugger_leave_sub(ctx);
            return Err(Error::Runtime(
                "invalid jump out of function (nextfile)".into(),
            ));
        }
        Ok(VmSignal::ExitPending) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            crate::vm::debugger_leave_sub(ctx);
            return Err(Error::Exit(ctx.rt.exit_code));
        }
        Err(e) => {
            ctx.locals.pop();
            ctx.in_function = was_fn;
            crate::vm::debugger_leave_sub(ctx);
            return Err(e);
        }
    };

    ctx.locals.pop();
    ctx.in_function = was_fn;
    crate::vm::debugger_leave_sub(ctx);
    Ok(result)
}
