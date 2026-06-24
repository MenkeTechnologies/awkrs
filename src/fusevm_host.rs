//! Host bridge for fusevm-native execution (migration stage 1).
//!
//! awkrs is migrating off its hand-written `vm.rs` interpreter onto fusevm's VM,
//! the way zshrs runs on fusevm. fusevm is built to host awk: it has first-class
//! `Op::Awk*` ops (field get/set, NF, print, getline, split, sub/gsub, …) that
//! dispatch to a [`fusevm::AwkHost`] trait. The fusevm-native awk path is to
//! implement that trait against awkrs's [`Runtime`], compile awk to those ops,
//! install the host, and run — no awkrs interpreter involved.
//!
//! fusevm's `AwkHost` methods take `&mut self` (the host), but the awk state
//! lives in [`Runtime`], which the surrounding executor owns. The host reaches
//! it through a thread-local pointer stashed for the duration of `vm.run()`,
//! mirroring zshrs's `CURRENT_EXECUTOR`. Ops move from `vm.rs` into the
//! [`AwkRuntimeHost`] impl one category at a time, each gated by the parity
//! suite, until `vm.rs` is dead and removed.

use std::cell::{Cell, RefCell};

use crate::error::Error;
use crate::runtime::Runtime;

thread_local! {
    /// Raw pointer to the awk [`Runtime`] currently driving a `vm.run()`.
    /// Set by [`RuntimeGuard::enter`] and cleared on its drop. The host reads it
    /// via [`with_runtime`]; it is never null while a guard is alive.
    static CURRENT_RT: Cell<*mut Runtime> = const { Cell::new(std::ptr::null_mut()) };

    /// First fatal awk error raised by the host during a `vm.run()`. The
    /// `AwkHost` methods return values, not `Result`, so a method that hits a
    /// fatal (e.g. a negative field index) records it here and the execution
    /// wrapper drains it with [`take_host_error`] after the run, turning it into
    /// the same `Err` the interpreter would have returned.
    static HOST_ERR: RefCell<Option<Error>> = const { RefCell::new(None) };
}

/// RAII guard exposing `rt` to the fusevm `AwkHost` for the guard's lifetime.
///
/// # Safety contract
/// While the guard is alive, `rt` must not be touched through its original
/// `&mut` — only through [`with_runtime`] (i.e. from inside host methods during
/// `vm.run()`). The guard brackets `vm.run()` exactly, so no aliasing occurs.
pub(crate) struct RuntimeGuard {
    _private: (),
}

impl RuntimeGuard {
    pub(crate) fn enter(rt: &mut Runtime) -> Self {
        CURRENT_RT.with(|c| c.set(rt as *mut Runtime));
        HOST_ERR.with(|c| *c.borrow_mut() = None);
        RuntimeGuard { _private: () }
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        CURRENT_RT.with(|c| c.set(std::ptr::null_mut()));
    }
}

/// Run `f` with the active awk [`Runtime`]. Panics if called outside a
/// [`RuntimeGuard`] scope (i.e. not from within the host during `vm.run()`).
fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut Runtime) -> R,
{
    CURRENT_RT.with(|c| {
        let ptr = c.get();
        assert!(
            !ptr.is_null(),
            "with_runtime called outside a RuntimeGuard scope"
        );
        // SAFETY: the pointer was set from a live `&mut Runtime` by
        // `RuntimeGuard::enter` and is cleared on guard drop; per the guard's
        // safety contract the Runtime is not aliased while the guard is alive.
        f(unsafe { &mut *ptr })
    })
}

/// Record a fatal awk error from inside the host. Only the first is kept, to
/// match the interpreter's fail-fast `?` propagation.
fn set_host_error(e: Error) {
    HOST_ERR.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            *slot = Some(e);
        }
    });
}

/// Drain the fatal error (if any) recorded during the last `vm.run()`. The
/// execution wrapper calls this after the run and propagates a `Some` as `Err`.
pub(crate) fn take_host_error() -> Option<Error> {
    HOST_ERR.with(|c| c.borrow_mut().take())
}

/// Install awkrs's hosts on a fresh fusevm VM, just before `vm.run()`: the
/// [`AwkHost`] for awk operations, and a minimal [`fusevm::ShellHost`] that backs
/// `Op::RegexMatch` (the `~` / `!~` operators) with awkrs's regex engine.
pub(crate) fn install_awk_host(vm: &mut fusevm::VM) {
    vm.set_awk_host(Box::new(AwkRuntimeHost));
    vm.set_shell_host(Box::new(AwkRegexHost));
    vm.register_builtin(BUILTIN_AWK_CMP, awk_cmp_builtin);
    vm.register_builtin(BUILTIN_AWK_KEYS, awk_keys_builtin);
    vm.register_builtin(BUILTIN_ARRAY_LEN, array_len_builtin);
    vm.register_builtin(BUILTIN_AWK_CONCAT, awk_concat_builtin);
}

/// String concatenation with awk's number→string rule: numbers stringify via
/// CONVFMT (so `CONVFMT="%.2f"; x=3.14159; x ""` is `"3.14"`), which fusevm's
/// native `Op::Concat` does not do.
pub(crate) const BUILTIN_AWK_CONCAT: u16 = 2003;

fn awk_concat_builtin(vm: &mut fusevm::VM, _argc: u8) -> fusevm::Value {
    let b = vm.pop();
    let a = vm.pop();
    let s = with_runtime(|rt| {
        let mut s = field_string(rt, &a);
        s.push_str(&field_string(rt, &b));
        s
    });
    fusevm::Value::str(s)
}

/// `for (k in a)` support: pop an array name and return its keys as a fusevm
/// `Array`, which the compiler iterates by index. awk leaves the order
/// unspecified, matching the runtime's hash order.
pub(crate) const BUILTIN_AWK_KEYS: u16 = 2001;
/// Pop a fusevm `Array` and return its element count (the keys-array length).
pub(crate) const BUILTIN_ARRAY_LEN: u16 = 2002;

fn awk_keys_builtin(vm: &mut fusevm::VM, _argc: u8) -> fusevm::Value {
    let name = vm.pop().to_str();
    let keys = with_runtime(|rt| rt.array_keys(&name));
    fusevm::Value::Array(keys.into_iter().map(fusevm::Value::str).collect())
}

fn array_len_builtin(vm: &mut fusevm::VM, _argc: u8) -> fusevm::Value {
    match vm.pop() {
        fusevm::Value::Array(a) => fusevm::Value::Int(a.len() as i64),
        _ => fusevm::Value::Int(0),
    }
}

/// Builtin id for the awk relational comparator. fusevm's `Value` model
/// (Int/Float/Str) cannot carry awk's strnum tag, so `==`/`<`/… are lowered to a
/// call to this builtin (returning -1/0/1 per awk's "both numeric → numeric, else
/// string" rule) followed by a numeric compare against 0.
pub(crate) const BUILTIN_AWK_CMP: u16 = 2000;

fn awk_cmp_builtin(vm: &mut fusevm::VM, _argc: u8) -> fusevm::Value {
    use std::cmp::Ordering;
    let b = vm.pop();
    let a = vm.pop();
    let ord = with_runtime(|rt| {
        crate::builtins::awk_value_sort_cmp_with_case(
            &fuse_to_awk(a),
            &fuse_to_awk(b),
            rt.ignore_case_flag(),
        )
    });
    fusevm::Value::Int(match ord {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

/// Backs fusevm's `Op::RegexMatch` for awk: a boolean `s ~ re` via the runtime's
/// compiled-regex cache. All other `ShellHost` methods keep their defaults (awk
/// never emits glob/tilde/param-expansion ops).
pub(crate) struct AwkRegexHost;

impl fusevm::ShellHost for AwkRegexHost {
    fn regex_match(&mut self, s: &str, regex: &str) -> bool {
        with_runtime(|rt| {
            if rt.ensure_regex(regex).is_err() {
                return false;
            }
            rt.regex_ref(regex).is_match(s)
        })
    }
}

/// awkrs's implementation of fusevm's awk operation surface. A zero-sized type:
/// all state lives in the thread-local [`Runtime`] reached via [`with_runtime`].
/// Methods not yet ported fall back to the trait's default (empty/no-op) and are
/// filled in as their ops migrate out of `vm.rs`.
pub(crate) struct AwkRuntimeHost;

impl fusevm::AwkHost for AwkRuntimeHost {
    /// `$i` — read field `i` (`$0` is the whole record). Port of `Op::GetField`.
    fn field_get(&mut self, i: i64) -> fusevm::Value {
        with_runtime(|rt| match rt.field(i as i32) {
            Ok(v) => awk_to_fuse(v),
            Err(e) => {
                set_host_error(e);
                fusevm::Value::str("")
            }
        })
    }

    /// `$i = v` — assign field `i`, rebuilding `$0`/`NF`. Port of `Op::SetField`
    /// (numbers stringify via CONVFMT for gawk parity).
    fn field_set(&mut self, i: i64, v: fusevm::Value) {
        with_runtime(|rt| {
            // Assigning `$n` (n>=1) materializes the existing fields first;
            // awkrs splits lazily, so realize the split here (idempotent) rather
            // than relying on a prior field read. `$0` resplits anyway.
            if i >= 1 {
                rt.ensure_fields_split();
            }
            let s = field_string(rt, &v);
            if let Err(e) = rt.set_field(i as i32, &s) {
                set_host_error(e);
            }
        });
    }

    /// `NF` — current field count.
    fn nf(&mut self) -> i64 {
        with_runtime(|rt| rt.nf() as i64)
    }

    /// `$0 = v` — replace the record and resplit on the current FS.
    fn set_record(&mut self, v: fusevm::Value) {
        with_runtime(|rt| {
            let s = v.to_str();
            if let Err(e) = rt.set_field(0, &s) {
                set_host_error(e);
            }
        });
    }

    /// Read a special variable (`NR`, `NF`, `FS`, `OFS`, `ORS`, `RS`, `SUBSEP`,
    /// `RSTART`, `RLENGTH`, `FILENAME`, `CONVFMT`, `OFMT`, …). Routes through the
    /// same resolver as gawk's `SYMTAB`, which synthesizes `NR`/`NF`/`FNR`/
    /// `FILENAME` and reads the rest from the global table.
    fn special_get(&mut self, name: &str) -> fusevm::Value {
        with_runtime(|rt| {
            // `NF` must realize the lazy field split first; `symtab_elem_get`
            // would report the unsplit (zero) count.
            if name == "NF" {
                return fusevm::Value::Int(rt.nf() as i64);
            }
            awk_to_fuse(rt.symtab_elem_get(name))
        })
    }

    /// Assign a special variable. `symtab_elem_set` keeps the `OFS`/`ORS` byte
    /// caches in sync and writes the global table (so a later `FS` change splits
    /// the next record, etc.).
    fn special_set(&mut self, name: &str, v: fusevm::Value) {
        with_runtime(|rt| {
            // Assigning `NF` truncates/extends the fields and rebuilds `$0`.
            if name == "NF" {
                if let Err(e) = rt.set_nf(fuse_num(&v) as i32) {
                    set_host_error(e);
                }
                return;
            }
            rt.symtab_elem_set(name, fuse_to_awk(v));
        });
    }

    // ── Arrays ────────────────────────────────────────────────────────────

    /// `a[k]` — read an array element (missing keys read as `""`, POSIX).
    fn array_get(&mut self, arr_name: &str, key: &fusevm::Value) -> fusevm::Value {
        with_runtime(|rt| {
            let k = array_key(rt, key);
            awk_to_fuse(rt.array_get(arr_name, &k))
        })
    }

    /// `a[k] = v` — assign an array element (auto-vivifying the array).
    fn array_set(&mut self, arr_name: &str, key: &fusevm::Value, v: fusevm::Value) {
        with_runtime(|rt| {
            let k = array_key(rt, key);
            rt.array_set(arr_name, k, fuse_to_awk(v));
        });
    }

    /// `k in a` — membership test (does not auto-vivify).
    fn array_exists(&mut self, arr_name: &str, key: &fusevm::Value) -> bool {
        with_runtime(|rt| {
            let k = array_key(rt, key);
            matches!(rt.get_global_var(arr_name), Some(crate::runtime::Value::Array(a)) if a.contains_key(&k))
        })
    }

    /// `delete a[k]` — remove one element.
    fn array_delete(&mut self, arr_name: &str, key: &fusevm::Value) {
        with_runtime(|rt| {
            let k = array_key(rt, key);
            rt.array_delete(arr_name, Some(&k));
        });
    }

    /// `delete a` — remove every element.
    fn array_clear(&mut self, arr_name: &str) {
        with_runtime(|rt| rt.array_delete(arr_name, None));
    }

    /// `length(a)` — element count (0 when `a` is unset or not an array).
    fn array_len(&mut self, arr_name: &str) -> i64 {
        with_runtime(|rt| match rt.get_global_var(arr_name) {
            Some(crate::runtime::Value::Array(a)) => a.len() as i64,
            _ => 0,
        })
    }

    // ── Numeric builtins (host fallback; the numeric JIT lowers these to
    //    native fusevm ops on the hot path) ────────────────────────────────

    /// `int(x)` — truncate toward zero.
    fn int(&mut self, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(x).trunc())
    }
    /// `sqrt(x)`.
    fn sqrt(&mut self, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(x).sqrt())
    }
    /// `sin(x)` (radians).
    fn sin(&mut self, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(x).sin())
    }
    /// `cos(x)` (radians).
    fn cos(&mut self, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(x).cos())
    }
    /// `exp(x)`.
    fn exp(&mut self, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(x).exp())
    }
    /// `log(x)` (natural log).
    fn log(&mut self, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(x).ln())
    }
    /// `atan2(y, x)`.
    fn atan2(&mut self, y: &fusevm::Value, x: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Float(fuse_num(y).atan2(fuse_num(x)))
    }

    // ── String builtins (ports of vm_builtins.rs) ─────────────────────────

    /// `length` / `length(s)` — char count (byte count under `-b`). `None` =
    /// `length` with no args, which measures `$0`. Port of the `length` arm.
    fn length(&mut self, s: Option<&fusevm::Value>) -> i64 {
        with_runtime(|rt| match s {
            None => {
                if rt.characters_as_bytes {
                    rt.record.len() as i64
                } else {
                    rt.record.chars().count() as i64
                }
            }
            Some(v) => {
                let s = v.to_str();
                if rt.characters_as_bytes {
                    s.len() as i64
                } else {
                    s.chars().count() as i64
                }
            }
        })
    }

    /// `index(hay, needle)` — 1-based position or 0. Honors IGNORECASE and `-b`.
    fn index(&mut self, s: &fusevm::Value, t: &fusevm::Value) -> i64 {
        with_runtime(|rt| {
            let hay = s.to_str();
            let needle = t.to_str();
            if needle.is_empty() {
                return 1;
            }
            let pos = if rt.ignore_case_flag() {
                let hay_lc = hay.to_lowercase();
                let needle_lc = needle.to_lowercase();
                hay_lc.find(needle_lc.as_str()).map(|b| {
                    if rt.characters_as_bytes {
                        b + 1
                    } else {
                        hay.char_indices().take_while(|&(off, _)| off < b).count() + 1
                    }
                })
            } else {
                hay.find(needle.as_str()).map(|b| {
                    if rt.characters_as_bytes {
                        b + 1
                    } else {
                        hay[..b].chars().count() + 1
                    }
                })
            };
            pos.map(|p| p as i64).unwrap_or(0)
        })
    }

    /// `substr(s, m [, n])` — 1-based; `m<1` clamps to 1, `n<=0` yields "".
    /// Port of the `substr` arm (m/n already integer-parsed by the VM).
    fn substr(&mut self, s: &fusevm::Value, m: i64, n: Option<i64>) -> fusevm::Value {
        with_runtime(|rt| {
            let s = s.to_str();
            let len = match n {
                Some(l) if l <= 0 => return fusevm::Value::str(""),
                Some(l) => l as usize,
                None => usize::MAX,
            };
            let mut m = m;
            if m < 1 {
                rt.lint_warn(&format!(
                    "substr: start index {m} is less than 1, treated as 1"
                ));
                m = 1;
            }
            let start0 = (m as usize).saturating_sub(1);
            let slice = if rt.characters_as_bytes {
                let b = s.as_bytes();
                b.get(start0..)
                    .map(|rest| {
                        let take = len.min(rest.len());
                        String::from_utf8_lossy(&rest[..take]).into_owned()
                    })
                    .unwrap_or_default()
            } else {
                s.chars().skip(start0).take(len).collect()
            };
            fusevm::Value::str(slice)
        })
    }

    /// `tolower(s)`. Port of the `tolower` arm.
    fn tolower(&mut self, s: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::str(s.to_str().to_lowercase())
    }

    /// `toupper(s)`. Port of the `toupper` arm.
    fn toupper(&mut self, s: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::str(s.to_str().to_uppercase())
    }

    // ── Bit ops + misc builtins (ports of vm_builtins.rs) ─────────────────

    /// `and(v1, v2, …)` — bitwise AND folded across all args (≥2 required).
    fn and(&mut self, args: &[fusevm::Value]) -> fusevm::Value {
        bit_fold(args, "and", crate::bignum::awk_and_values)
    }

    /// `or(v1, v2, …)` — bitwise OR folded across all args.
    fn or(&mut self, args: &[fusevm::Value]) -> fusevm::Value {
        bit_fold(args, "or", crate::bignum::awk_or_values)
    }

    /// `xor(v1, v2, …)` — bitwise XOR folded across all args.
    fn xor(&mut self, args: &[fusevm::Value]) -> fusevm::Value {
        bit_fold(args, "xor", crate::bignum::awk_xor_values)
    }

    /// `lshift(v, n)` — left shift; negative operands are fatal (gawk).
    fn lshift(&mut self, v: &fusevm::Value, n: &fusevm::Value) -> fusevm::Value {
        shift("lshift", v, n, crate::bignum::awk_lshift_values)
    }

    /// `rshift(v, n)` — right shift; negative operands are fatal (gawk).
    fn rshift(&mut self, v: &fusevm::Value, n: &fusevm::Value) -> fusevm::Value {
        shift("rshift", v, n, crate::bignum::awk_rshift_values)
    }

    /// `compl(v)` — bitwise complement; negative operand is fatal (gawk).
    fn compl(&mut self, v: &fusevm::Value) -> fusevm::Value {
        with_runtime(|rt| {
            let av = fuse_num(v);
            if av < 0.0 {
                set_host_error(Error::Runtime(format!(
                    "compl({av:.6}): negative value is not allowed"
                )));
                return fusevm::Value::Int(0);
            }
            awk_to_fuse(crate::bignum::awk_compl_values(&fuse_to_awk(v.clone()), rt))
        })
    }

    /// `strtonum(s)` — numeric value honoring 0x/0 prefixes (gawk).
    fn strtonum(&mut self, s: &fusevm::Value) -> fusevm::Value {
        with_runtime(|rt| awk_to_fuse(crate::bignum::awk_strtonum_value(&s.to_str(), rt)))
    }

    /// `intdiv(a, b)` — integer division; div-by-zero is fatal.
    fn intdiv(&mut self, a: &fusevm::Value, b: &fusevm::Value) -> fusevm::Value {
        with_runtime(|rt| {
            match crate::bignum::awk_intdiv_values(
                &fuse_to_awk(a.clone()),
                &fuse_to_awk(b.clone()),
                rt,
            ) {
                Ok(v) => awk_to_fuse(v),
                Err(e) => {
                    set_host_error(e);
                    fusevm::Value::Int(0)
                }
            }
        })
    }

    /// `mkbool(expr)` — 1 if truthy (numeric != 0, including NaN/inf; non-empty
    /// string), else 0 (gawk).
    fn mkbool(&mut self, arg: &fusevm::Value) -> fusevm::Value {
        fusevm::Value::Int(if awk_truthy(arg) { 1 } else { 0 })
    }

    /// `ord(s)` — code point of the first character of `s`.
    fn ord(&mut self, arg: &fusevm::Value) -> fusevm::Value {
        with_runtime(|rt| match crate::gawk_extensions::ord(rt, &arg.to_str()) {
            Ok(v) => awk_to_fuse(v),
            Err(e) => {
                set_host_error(e);
                fusevm::Value::Int(0)
            }
        })
    }

    /// `chr(n)` — character for code point `n`.
    fn chr(&mut self, arg: &fusevm::Value) -> fusevm::Value {
        with_runtime(|rt| match crate::gawk_extensions::chr(rt, fuse_num(arg)) {
            Ok(v) => awk_to_fuse(v),
            Err(e) => {
                set_host_error(e);
                fusevm::Value::str("")
            }
        })
    }

    /// `sub(re, repl, target)` — replace the first match in `target`; returns
    /// 1/0. Port of `exec_sub` (non-global).
    fn sub(
        &mut self,
        re: &fusevm::Value,
        repl: &fusevm::Value,
        target_ref: &fusevm::AwkLvalue,
    ) -> i64 {
        host_substitute(re, repl, target_ref, false)
    }

    /// `gsub(re, repl, target)` — replace all matches in `target`; returns the
    /// count. Port of `exec_sub` (global).
    fn gsub(
        &mut self,
        re: &fusevm::Value,
        repl: &fusevm::Value,
        target_ref: &fusevm::AwkLvalue,
    ) -> i64 {
        host_substitute(re, repl, target_ref, true)
    }

    /// `match(s, re)` — set RSTART/RLENGTH to the match position/length and return
    /// the 1-based position (0 if no match). Port of `Op::MatchBuiltin`.
    fn match_re(&mut self, s: &fusevm::Value, re: &fusevm::Value) -> i64 {
        with_runtime(
            |rt| match crate::builtins::match_fn(rt, &s.to_str(), &re.to_str(), None) {
                Ok(r) => r as i64,
                Err(e) => {
                    set_host_error(e);
                    0
                }
            },
        )
    }

    /// `split(s, arr [, fs])` — split `s` into `arr` and return the field count.
    /// `fs` defaults to the current `FS`. Port of `Op::Split` (seps array omitted —
    /// the 4-arg form is a separate op).
    fn split(&mut self, s: &fusevm::Value, arr_name: &str, fs: Option<&fusevm::Value>) -> i64 {
        with_runtime(|rt| {
            let fs_str = match fs {
                Some(v) => v.to_str(),
                None => rt
                    .vars
                    .get("FS")
                    .map(|v| v.as_str())
                    .unwrap_or_else(|| " ".to_string()),
            };
            let (parts, _seps) =
                crate::runtime::split_string_with_seps(&s.to_str(), &fs_str, rt.ignore_case_flag());
            let n = parts.len();
            rt.split_into_array(arr_name, &parts);
            n as i64
        })
    }

    // ── Time builtins + comparison ────────────────────────────────────────

    /// `systime()` — seconds since the epoch.
    fn systime(&mut self) -> fusevm::Value {
        fusevm::Value::Float(crate::builtins::awk_systime())
    }

    /// `strftime([fmt [, ts [, utc]]])` — format a timestamp. Port of the arm.
    fn strftime(&mut self, args: &[fusevm::Value]) -> fusevm::Value {
        let vals: Vec<crate::runtime::Value> =
            args.iter().map(|a| fuse_to_awk(a.clone())).collect();
        match crate::builtins::awk_strftime(&vals) {
            Ok(v) => awk_to_fuse(v),
            Err(e) => {
                set_host_error(Error::Runtime(e));
                fusevm::Value::str("")
            }
        }
    }

    /// `mktime(datespec [, utc])` — convert a "YYYY MM DD HH MM SS [DST]" spec to
    /// a timestamp; truthy second arg interprets it as UTC. Port of the arm.
    fn mktime(&mut self, args: &[fusevm::Value]) -> fusevm::Value {
        if args.is_empty() {
            return fusevm::Value::Float(-1.0);
        }
        let utc = args.len() >= 2 && fuse_num(&args[1]) != 0.0;
        fusevm::Value::Float(crate::builtins::awk_mktime_with_utc(&args[0].to_str(), utc))
    }

    /// `getline` family. Non-VAR forms (`getline`, `getline < file`,
    /// `cmd | getline`) read the next record into `$0` and update NF (+ NR/FNR for
    /// main input), mirroring `apply_getline_line`'s no-var path. Returns 1 (read),
    /// 0 (EOF), -1 (error).
    ///
    /// The `*_VAR` forms (`getline var`, …) cannot be served: fusevm 0.13.9's
    /// `AWK_GETLINE` dispatch passes `var_name = None`, so the target variable is
    /// not threaded to the host. That gap belongs upstream in fusevm; rather than
    /// silently drop the line, this records a fatal error.
    fn getline(&mut self, source: usize, operand: Option<&str>, _var_name: Option<&str>) -> i64 {
        use fusevm::awk_builtins::getline_source as gs;
        with_runtime(|rt| {
            if matches!(source, gs::MAIN_VAR | gs::FILE_VAR | gs::CMD_VAR) {
                set_host_error(Error::Runtime(
                    "getline into a variable is not yet supported on fusevm \
                     (the AWK_GETLINE dispatch does not thread the target var)"
                        .into(),
                ));
                return -1;
            }
            let line_res = match source {
                gs::MAIN => rt.read_line_primary(),
                gs::FILE => rt.read_line_file(operand.unwrap_or("")),
                gs::CMD => rt.read_line_pipe(operand.unwrap_or("")),
                _ => return -1,
            };
            match line_res {
                Ok(Some(l)) => {
                    let trimmed = l.trim_end_matches(['\n', '\r']).to_string();
                    let fs = rt
                        .vars
                        .get("FS")
                        .map(|v| v.as_str())
                        .unwrap_or_else(|| " ".to_string());
                    rt.set_field_sep_split(&fs, &trimmed);
                    rt.ensure_fields_split();
                    let nf = rt.nf() as f64;
                    rt.vars.insert("NF".into(), crate::runtime::Value::Num(nf));
                    if source == gs::MAIN {
                        rt.nr += 1.0;
                        rt.fnr += 1.0;
                    }
                    1
                }
                Ok(None) => 0,
                Err(e) => {
                    // Sandbox redirection violations stay fatal (gawk parity).
                    if matches!(&e, Error::Runtime(msg) if msg.starts_with("sandbox:")) {
                        set_host_error(e);
                    }
                    -1
                }
            }
        })
    }

    /// Total ordering of two values for `asort`/`asorti`, honoring IGNORECASE.
    fn compare(&mut self, a: &fusevm::Value, b: &fusevm::Value) -> std::cmp::Ordering {
        with_runtime(|rt| {
            crate::builtins::awk_value_sort_cmp_with_case(
                &fuse_to_awk(a.clone()),
                &fuse_to_awk(b.clone()),
                rt.ignore_case_flag(),
            )
        })
    }

    /// `gensub(re, repl, how [, target])` — non-destructive substitution returning
    /// the new string (`target` defaults to `$0`). Port of the `gensub` arm.
    fn gensub(
        &mut self,
        re: &fusevm::Value,
        repl: &fusevm::Value,
        how: &fusevm::Value,
        target: Option<&fusevm::Value>,
    ) -> fusevm::Value {
        with_runtime(|rt| {
            let how_awk = fuse_to_awk(how.clone());
            let target_s = target.map(|t| t.to_str());
            match crate::builtins::awk_gensub(rt, &re.to_str(), &repl.to_str(), &how_awk, target_s)
            {
                Ok(s) => fusevm::Value::str(s),
                Err(e) => {
                    set_host_error(e);
                    fusevm::Value::str("")
                }
            }
        })
    }

    /// `print a, b, …` — args joined by OFS, terminated by ORS, to the record
    /// stream. Numbers render via OFMT (POSIX). Port of `Op::Print`.
    fn print(&mut self, args: &[fusevm::Value]) {
        with_runtime(|rt| {
            let ofs = rt.ofs_bytes.clone();
            let ors = rt.ors_bytes.clone();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    rt.print_buf.extend_from_slice(&ofs);
                }
                let s = output_string(rt, a);
                rt.print_buf.extend_from_slice(s.as_bytes());
            }
            rt.print_buf.extend_from_slice(&ors);
        });
    }

    /// `printf fmt, …` — format and emit (no trailing ORS). Port of the `printf`
    /// arm via the shared format engine.
    fn printf(&mut self, fmt: &str, args: &[fusevm::Value]) {
        with_runtime(|rt| match run_sprintf(rt, fmt, args) {
            Ok(s) => rt.print_buf.extend_from_slice(s.as_bytes()),
            Err(e) => set_host_error(e),
        });
    }

    /// `sprintf(fmt, …)` — format and return the string. Port of the `sprintf` arm.
    fn sprintf(&mut self, fmt: &str, args: &[fusevm::Value]) -> fusevm::Value {
        with_runtime(|rt| match run_sprintf(rt, fmt, args) {
            Ok(s) => fusevm::Value::str(s),
            Err(e) => {
                set_host_error(e);
                fusevm::Value::str("")
            }
        })
    }
}

/// awk [`crate::runtime::Value`] → fusevm `Value` for results pushed back.
fn awk_to_fuse(v: crate::runtime::Value) -> fusevm::Value {
    use crate::runtime::Value as A;
    match v {
        A::Uninit => fusevm::Value::Undef,
        A::Str(s) | A::StrLit(s) | A::Regexp(s) => fusevm::Value::str(s),
        A::Num(n) => fusevm::Value::Float(n),
        A::Mpfr(f) => fusevm::Value::Float(f.to_f64()),
        A::Array(_) => fusevm::Value::Undef,
    }
}

/// fusevm `Value` → awk [`crate::runtime::Value`] for assignments into the
/// Runtime (special vars, arrays, fields). Numeric kinds become `Num`; awk has
/// no distinct boolean, so `Bool` collapses to `1`/`0`.
fn fuse_to_awk(v: fusevm::Value) -> crate::runtime::Value {
    use crate::runtime::Value as A;
    match v {
        fusevm::Value::Undef => A::Uninit,
        fusevm::Value::Bool(b) => A::Num(if b { 1.0 } else { 0.0 }),
        fusevm::Value::Int(n) => A::Num(n as f64),
        fusevm::Value::Float(f) => A::Num(f),
        fusevm::Value::Str(s) => A::Str((*s).clone()),
        fusevm::Value::Status(n) => A::Num(n as f64),
        fusevm::Value::Ref(inner) => fuse_to_awk(*inner),
        fusevm::Value::Array(_) | fusevm::Value::Hash(_) | fusevm::Value::NativeFn(_) => A::Uninit,
    }
}

/// fusevm `Value` → f64 for numeric builtins (leading-numeric parse for strings).
fn fuse_num(v: &fusevm::Value) -> f64 {
    match v {
        fusevm::Value::Float(f) => *f,
        fusevm::Value::Int(n) => *n as f64,
        fusevm::Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        fusevm::Value::Status(n) => *n as f64,
        fusevm::Value::Str(s) => s.trim().parse::<f64>().unwrap_or(0.0),
        fusevm::Value::Ref(inner) => fuse_num(inner),
        _ => 0.0,
    }
}

/// Build an array key string from a fusevm index value, honoring CONVFMT for
/// numeric subscripts (POSIX) via the Runtime's canonical key formatter.
fn array_key(rt: &Runtime, key: &fusevm::Value) -> String {
    rt.value_to_array_key(&fuse_to_awk(key.clone()))
}

/// Fold a 2-arg bitwise op across `args` (`and`/`or`/`xor`); ≥2 args required.
fn bit_fold(
    args: &[fusevm::Value],
    name: &str,
    op: fn(&crate::runtime::Value, &crate::runtime::Value, &Runtime) -> crate::runtime::Value,
) -> fusevm::Value {
    with_runtime(|rt| {
        if args.len() < 2 {
            set_host_error(Error::Runtime(format!(
                "{name}: called with less than two arguments"
            )));
            return fusevm::Value::Int(0);
        }
        let mut acc = op(
            &fuse_to_awk(args[0].clone()),
            &fuse_to_awk(args[1].clone()),
            rt,
        );
        for a in &args[2..] {
            acc = op(&acc, &fuse_to_awk(a.clone()), rt);
        }
        awk_to_fuse(acc)
    })
}

/// `lshift`/`rshift`: two args, both non-negative (negative is a gawk fatal).
fn shift(
    name: &str,
    v: &fusevm::Value,
    n: &fusevm::Value,
    op: fn(&crate::runtime::Value, &crate::runtime::Value, &Runtime) -> crate::runtime::Value,
) -> fusevm::Value {
    with_runtime(|rt| {
        let av = fuse_num(v);
        let bv = fuse_num(n);
        if av < 0.0 || bv < 0.0 {
            set_host_error(Error::Runtime(format!(
                "{name}({av:.6}, {bv:.6}): negative values are not allowed"
            )));
            return fusevm::Value::Int(0);
        }
        awk_to_fuse(op(&fuse_to_awk(v.clone()), &fuse_to_awk(n.clone()), rt))
    })
}

/// Format `args` per `fmt` through awkrs's format engine, with the runtime's
/// CONVFMT / decimal-point / thousands-sep / bignum settings — the same wiring
/// as `vm.rs::sprintf_simple`, so output matches the interpreter byte for byte.
fn run_sprintf(rt: &Runtime, fmt: &str, args: &[fusevm::Value]) -> Result<String, Error> {
    let vals: Vec<crate::runtime::Value> = args.iter().map(|a| fuse_to_awk(a.clone())).collect();
    let mpfr = rt.bignum.then(|| (rt.mpfr_prec_bits(), rt.mpfr_round()));
    let convfmt = rt
        .get_global_var("CONVFMT")
        .map(|v| v.as_str())
        .unwrap_or_else(|| "%.6g".to_string());
    crate::format::awk_sprintf_with_convfmt(
        fmt,
        &vals,
        rt.numeric_decimal,
        rt.numeric_thousands_sep,
        mpfr,
        &convfmt,
    )
    .map_err(Error::Runtime)
}

/// Read-modify-write `sub`/`gsub` over an `AwkLvalue` target. Mirrors `exec_sub`:
/// `$0` substitutes on the record in place; `$n` / var / array-elem read the
/// current string, substitute, and write it back. Returns the replacement count.
fn host_substitute(
    re: &fusevm::Value,
    repl: &fusevm::Value,
    target: &fusevm::AwkLvalue,
    global: bool,
) -> i64 {
    with_runtime(|rt| {
        let re_s = re.to_str();
        let repl_s = repl.to_str();
        let sub_call: fn(&mut Runtime, &str, &str, Option<&mut String>) -> Result<f64, Error> =
            if global {
                crate::builtins::gsub
            } else {
                crate::builtins::sub_fn
            };

        let res: Result<f64, Error> = match target {
            // `$0` — substitute on the record directly (resplits), like SubTarget::Record.
            fusevm::AwkLvalue::Field(0) => sub_call(rt, &re_s, &repl_s, None),
            fusevm::AwkLvalue::Field(i) => match rt.field(*i as i32) {
                Ok(v) => {
                    let mut s = v.as_str();
                    match sub_call(rt, &re_s, &repl_s, Some(&mut s)) {
                        Ok(n) => rt.set_field(*i as i32, &s).map(|()| n),
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            },
            fusevm::AwkLvalue::Var(name) => {
                let mut s = rt.symtab_elem_get(name).as_str();
                let n = sub_call(rt, &re_s, &repl_s, Some(&mut s));
                if n.is_ok() {
                    rt.symtab_elem_set(name, crate::runtime::Value::Str(s));
                }
                n
            }
            fusevm::AwkLvalue::ArrayElem(name, key) => {
                let mut s = rt.array_get(name, key).as_str();
                let n = sub_call(rt, &re_s, &repl_s, Some(&mut s));
                if n.is_ok() {
                    rt.array_set(name, key.clone(), crate::runtime::Value::Str(s));
                }
                n
            }
        };

        match res {
            Ok(n) => n as i64,
            Err(e) => {
                set_host_error(e);
                0
            }
        }
    })
}

/// awk truthiness of a fusevm value: numbers are true when non-zero (NaN/inf
/// included), strings when non-empty, undef false.
fn awk_truthy(v: &fusevm::Value) -> bool {
    match v {
        fusevm::Value::Undef => false,
        fusevm::Value::Bool(b) => *b,
        fusevm::Value::Int(n) => *n != 0,
        fusevm::Value::Float(f) => *f != 0.0,
        fusevm::Value::Str(s) => !s.is_empty(),
        fusevm::Value::Status(n) => *n != 0,
        fusevm::Value::Ref(inner) => awk_truthy(inner),
        _ => false,
    }
}

/// Booleans (from `~`/`in`/comparisons) render as awk's numeric `1`/`0`.
fn bool_str(b: bool) -> String {
    if b {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

/// fusevm `Value` → string for `print` output (numbers via OFMT).
fn output_string(rt: &Runtime, v: &fusevm::Value) -> String {
    match v {
        fusevm::Value::Float(n) => rt.num_to_string_ofmt(*n),
        fusevm::Value::Int(n) => n.to_string(),
        fusevm::Value::Bool(b) => bool_str(*b),
        fusevm::Value::Str(s) => (**s).clone(),
        fusevm::Value::Undef => String::new(),
        other => other.to_str(),
    }
}

/// fusevm `Value` → string for a field assignment (numbers via CONVFMT).
fn field_string(rt: &Runtime, v: &fusevm::Value) -> String {
    match v {
        fusevm::Value::Float(n) => rt.num_to_string_convfmt(*n),
        fusevm::Value::Int(n) => n.to_string(),
        fusevm::Value::Bool(b) => bool_str(*b),
        fusevm::Value::Str(s) => (**s).clone(),
        fusevm::Value::Undef => String::new(),
        other => other.to_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusevm::AwkHost as _;

    #[test]
    fn with_runtime_outside_guard_panics() {
        let r = std::panic::catch_unwind(|| with_runtime(|_| 1));
        assert!(r.is_err(), "with_runtime must panic with no active guard");
    }

    #[test]
    fn host_field_get_reads_fields_and_record() {
        let mut rt = Runtime::new();
        rt.set_field_sep_split(" ", "alpha beta gamma");
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        assert_eq!(host.field_get(2).to_str(), "beta");
        assert_eq!(host.field_get(0).to_str(), "alpha beta gamma");
        assert_eq!(host.nf(), 3);
    }

    #[test]
    fn host_field_set_assigns_and_rebuilds_record() {
        let mut rt = Runtime::new();
        rt.set_field_sep_split(" ", "a b c");
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let mut host = AwkRuntimeHost;
            // awkrs splits fields lazily; a field/NF access realizes the split,
            // exactly as it is during record processing before any assignment.
            let _ = host.nf();
            host.field_set(2, fusevm::Value::str("X"));
        }
        assert_eq!(rt.field(2).unwrap().as_str(), "X");
        assert_eq!(rt.field(0).unwrap().as_str(), "a X c");
    }

    #[test]
    fn host_print_joins_ofs_terminates_ors() {
        let mut rt = Runtime::new();
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let mut host = AwkRuntimeHost;
            host.print(&[fusevm::Value::str("a"), fusevm::Value::str("b")]);
        }
        // Default OFS = " ", ORS = "\n".
        assert_eq!(rt.print_buf, b"a b\n");
    }

    #[test]
    fn negative_field_index_records_host_error() {
        let mut rt = Runtime::new();
        rt.set_field_sep_split(" ", "a b");
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let mut host = AwkRuntimeHost;
            let _ = host.field_get(-1);
        }
        assert!(
            take_host_error().is_some(),
            "negative field index must record a fatal host error"
        );
    }

    #[test]
    fn host_special_get_reads_nr_nf_and_globals() {
        let mut rt = Runtime::new();
        rt.set_field_sep_split(" ", "a b c");
        rt.nr = 7.0;
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let _ = host.nf(); // realize the split so NF reflects 3
        assert_eq!(host.special_get("NR").to_str(), "7");
        assert_eq!(host.special_get("NF").to_int(), 3);
        // FS defaults to a single space.
        assert_eq!(host.special_get("FS").to_str(), " ");
    }

    #[test]
    fn host_nf_read_realizes_split_and_assign_rebuilds() {
        let mut rt = Runtime::new();
        rt.set_field_sep_split(" ", "a b c d");
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        // NF read must realize the lazy split (not report 0).
        assert_eq!(host.special_get("NF").to_int(), 4);
        // NF assignment truncates the record and rebuilds $0.
        host.special_set("NF", fusevm::Value::Int(2));
        assert_eq!(host.special_get("NF").to_int(), 2);
        assert_eq!(host.field_get(0).to_str(), "a b");
    }

    #[test]
    fn host_special_set_updates_ofs_and_globals() {
        let mut rt = Runtime::new();
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let mut host = AwkRuntimeHost;
            host.special_set("OFS", fusevm::Value::str("-"));
            host.special_set("SUBSEP", fusevm::Value::str(":"));
        }
        // OFS byte cache is what `print` joins with — verify it took effect.
        assert_eq!(rt.ofs_bytes, b"-");
        assert_eq!(rt.symtab_elem_get("SUBSEP").as_str(), ":");
    }

    #[test]
    fn host_array_set_get_exists_delete_len() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;

        host.array_set("a", &fusevm::Value::str("k1"), fusevm::Value::str("v1"));
        host.array_set("a", &fusevm::Value::Int(2), fusevm::Value::str("v2"));
        assert_eq!(
            host.array_get("a", &fusevm::Value::str("k1")).to_str(),
            "v1"
        );
        // numeric subscript 2 stringifies to "2"
        assert_eq!(host.array_get("a", &fusevm::Value::Int(2)).to_str(), "v2");
        assert!(host.array_exists("a", &fusevm::Value::str("k1")));
        assert!(!host.array_exists("a", &fusevm::Value::str("nope")));
        assert_eq!(host.array_len("a"), 2);

        host.array_delete("a", &fusevm::Value::str("k1"));
        assert!(!host.array_exists("a", &fusevm::Value::str("k1")));
        assert_eq!(host.array_len("a"), 1);

        host.array_clear("a");
        assert_eq!(host.array_len("a"), 0);
    }

    #[test]
    fn host_math_builtins() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        assert_eq!(fuse_num(&host.int(&fusevm::Value::Float(3.9))), 3.0);
        assert_eq!(fuse_num(&host.int(&fusevm::Value::Float(-3.9))), -3.0);
        assert_eq!(fuse_num(&host.sqrt(&fusevm::Value::Float(9.0))), 3.0);
        let e1 = host.exp(&fusevm::Value::Float(1.0));
        assert!((fuse_num(&host.log(&e1)) - 1.0).abs() < 1e-12);
        assert_eq!(
            fuse_num(&host.atan2(&fusevm::Value::Float(0.0), &fusevm::Value::Float(1.0))),
            0.0
        );
    }

    #[test]
    fn host_string_builtins() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let hello = fusevm::Value::str("hello world");
        assert_eq!(host.length(Some(&hello)), 11);
        assert_eq!(host.index(&hello, &fusevm::Value::str("world")), 7);
        assert_eq!(host.index(&hello, &fusevm::Value::str("xyz")), 0);
        assert_eq!(host.substr(&hello, 7, None).to_str(), "world");
        assert_eq!(host.substr(&hello, 1, Some(5)).to_str(), "hello");
        // m < 1 clamps to 1 (POSIX/gawk).
        assert_eq!(host.substr(&hello, -3, Some(5)).to_str(), "hello");
        assert_eq!(host.tolower(&fusevm::Value::str("AbC")).to_str(), "abc");
        assert_eq!(host.toupper(&fusevm::Value::str("AbC")).to_str(), "ABC");
    }

    #[test]
    fn host_bit_and_misc_builtins() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let i = |n: i64| fusevm::Value::Int(n);
        assert_eq!(fuse_num(&host.and(&[i(12), i(10)])), 8.0); // 1100 & 1010 = 1000
        assert_eq!(fuse_num(&host.or(&[i(12), i(10)])), 14.0); // 1100 | 1010 = 1110
        assert_eq!(fuse_num(&host.xor(&[i(12), i(10)])), 6.0); // 1100 ^ 1010 = 0110
        assert_eq!(fuse_num(&host.lshift(&i(1), &i(4))), 16.0);
        assert_eq!(fuse_num(&host.rshift(&i(16), &i(2))), 4.0);
        assert_eq!(fuse_num(&host.intdiv(&i(17), &i(5))), 3.0);
        assert_eq!(fuse_num(&host.strtonum(&fusevm::Value::str("0x1f"))), 31.0);
        assert_eq!(fuse_num(&host.mkbool(&i(0))), 0.0);
        assert_eq!(fuse_num(&host.mkbool(&fusevm::Value::str("x"))), 1.0);
    }

    #[test]
    fn host_printf_sprintf_format() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let s = host.sprintf(
            "%s=%d (%.2f)",
            &[
                fusevm::Value::str("x"),
                fusevm::Value::Int(42),
                fusevm::Value::Float(3.14159),
            ],
        );
        assert_eq!(s.to_str(), "x=42 (3.14)");
    }

    #[test]
    fn host_printf_writes_no_trailing_ors() {
        let mut rt = Runtime::new();
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let mut host = AwkRuntimeHost;
            host.printf("%d-%d", &[fusevm::Value::Int(1), fusevm::Value::Int(2)]);
        }
        assert_eq!(rt.print_buf, b"1-2"); // printf adds no ORS
    }

    #[test]
    fn host_gsub_and_sub_on_lvalues() {
        let mut rt = Runtime::new();
        rt.set_field_sep_split(" ", "foo bar");
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let _ = host.nf(); // realize the field split

        // gsub on a global var
        host.special_set("x", fusevm::Value::str("foofoo"));
        let n = host.gsub(
            &fusevm::Value::str("o"),
            &fusevm::Value::str("0"),
            &fusevm::AwkLvalue::Var("x".to_string()),
        );
        assert_eq!(n, 4);
        assert_eq!(host.special_get("x").to_str(), "f00f00");

        // gsub on $1 (field, write-back rebuilds the record)
        let n2 = host.gsub(
            &fusevm::Value::str("o"),
            &fusevm::Value::str("0"),
            &fusevm::AwkLvalue::Field(1),
        );
        assert_eq!(n2, 2);
        assert_eq!(host.field_get(1).to_str(), "f00");

        // sub (first match only) on $0
        let n3 = host.sub(
            &fusevm::Value::str("a"),
            &fusevm::Value::str("A"),
            &fusevm::AwkLvalue::Field(0),
        );
        assert_eq!(n3, 1);
        assert_eq!(host.field_get(0).to_str(), "f00 bAr");
    }

    #[test]
    fn host_getline_var_form_surfaces_fusevm_gap() {
        use fusevm::awk_builtins::getline_source as gs;
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        // `getline var` cannot be served (fusevm passes no var target): -1 + error.
        let r = host.getline(gs::MAIN_VAR, None, None);
        assert_eq!(r, -1);
        assert!(take_host_error().is_some());
    }

    #[test]
    fn host_getline_file_missing_returns_minus_one() {
        use fusevm::awk_builtins::getline_source as gs;
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        // Reading from a nonexistent file yields the getline error result (-1),
        // not a fatal — matching gawk's expression-form `getline < file`.
        let r = host.getline(gs::FILE, Some("/no/such/awkrs/file/xyz"), None);
        assert_eq!(r, -1);
    }

    #[test]
    fn host_time_and_compare() {
        use std::cmp::Ordering;
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        // systime is a positive epoch second
        assert!(fuse_num(&host.systime()) > 1_600_000_000.0);
        // strftime with an explicit format + timestamp (UTC) is deterministic
        let s = host.strftime(&[
            fusevm::Value::str("%Y-%m-%d"),
            fusevm::Value::Int(0),
            fusevm::Value::Int(1),
        ]);
        assert_eq!(s.to_str(), "1970-01-01");
        // numeric comparison: 2 < 10 numerically (not lexically)
        assert_eq!(
            host.compare(&fusevm::Value::Int(2), &fusevm::Value::Int(10)),
            Ordering::Less
        );
        // string comparison for non-numeric
        assert_eq!(
            host.compare(&fusevm::Value::str("b"), &fusevm::Value::str("a")),
            Ordering::Greater
        );
    }

    #[test]
    fn host_match_sets_rstart_rlength() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let pos = host.match_re(
            &fusevm::Value::str("hello world"),
            &fusevm::Value::str("wor"),
        );
        assert_eq!(pos, 7);
        assert_eq!(host.special_get("RSTART").to_int(), 7);
        assert_eq!(host.special_get("RLENGTH").to_int(), 3);
        // no match → 0, RSTART 0, RLENGTH -1
        assert_eq!(
            host.match_re(&fusevm::Value::str("abc"), &fusevm::Value::str("xyz")),
            0
        );
        assert_eq!(host.special_get("RLENGTH").to_int(), -1);
    }

    #[test]
    fn host_split_into_array() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let n = host.split(
            &fusevm::Value::str("a:b:c"),
            "arr",
            Some(&fusevm::Value::str(":")),
        );
        assert_eq!(n, 3);
        assert_eq!(host.array_get("arr", &fusevm::Value::Int(1)).to_str(), "a");
        assert_eq!(host.array_get("arr", &fusevm::Value::Int(2)).to_str(), "b");
        assert_eq!(host.array_get("arr", &fusevm::Value::Int(3)).to_str(), "c");
    }

    #[test]
    fn host_gensub_global_substitution() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        // gensub(/o/, "0", "g", "foo") → "f00"
        let out = host.gensub(
            &fusevm::Value::str("o"),
            &fusevm::Value::str("0"),
            &fusevm::Value::str("g"),
            Some(&fusevm::Value::str("foo")),
        );
        assert_eq!(out.to_str(), "f00");
        // Numeric how=1 replaces only the first match (string how must be g/G).
        let first = host.gensub(
            &fusevm::Value::str("o"),
            &fusevm::Value::str("0"),
            &fusevm::Value::Int(1),
            Some(&fusevm::Value::str("foo")),
        );
        assert_eq!(first.to_str(), "f0o");
    }

    #[test]
    fn host_negative_shift_is_fatal() {
        let mut rt = Runtime::new();
        let _g = RuntimeGuard::enter(&mut rt);
        let mut host = AwkRuntimeHost;
        let _ = host.lshift(&fusevm::Value::Int(-1), &fusevm::Value::Int(2));
        assert!(take_host_error().is_some());
    }

    /// End-to-end through fusevm's VM: a compiled chunk emitting fusevm's
    /// first-class `Op::AwkPrint` dispatches through the installed host into the
    /// Runtime's print buffer — the full fusevm-native path, no `vm.rs`.
    #[test]
    fn vm_dispatches_awk_print_through_host() {
        let mut b = fusevm::ChunkBuilder::new();
        let c = b.add_constant(fusevm::Value::str("hi"));
        b.emit(fusevm::Op::LoadConst(c), 0);
        b.emit(fusevm::Op::AwkPrint(1), 0);
        let mut vm = fusevm::VM::new(b.build());
        install_awk_host(&mut vm);

        let mut rt = Runtime::new();
        {
            let _g = RuntimeGuard::enter(&mut rt);
            let _ = vm.run();
        }
        assert_eq!(rt.print_buf, b"hi\n");
    }
}
