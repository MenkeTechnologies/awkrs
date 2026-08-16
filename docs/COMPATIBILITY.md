# awkrs compatibility vs BSD awk, mawk, and gawk

This document is a **feature matrix**, not a proof of correctness. **awkrs does not claim** bit-identical behavior, zero defects, or complete coverage of every extension in three other implementations. Where behavior is **unspecified by POSIX** (random number sequences, hash iteration order, subtle `printf` rounding), differences are expected.

**Legend**

| Cell | Meaning |
|------|---------|
| **Match** | Intended to follow the reference; covered by tests or explicit design. |
| **Part** | Subset, different edge cases, or alternate diagnostics. |
| **Ext** | Extension in that engine; POSIX `awk` may lack it. |
| **No** | Not supported or incompatible. |
| **—** | Not applicable. |

References: special variables and builtins lists in `src/compiler.rs` (`SPECIAL_VARS`) and `src/namespace.rs` (`BUILTIN_NAMES`, `SPECIAL_GLOBAL_NAMES`). CLI surface in `src/cli.rs`.

---

## 1. Executive summary

| Topic | awkrs stance |
|-------|----------------|
| POSIX core | Large subset implemented; `-P`/`posix` toggles some ordering rules (e.g. `for (i in a)` without gawk-style `PROCINFO["sorted_in"]` sorting). |
| BSD awk (e.g. `nawk`) | Many **gawk-only** features in awkrs are **not** in BSD awk; matrix below marks **Ext** for gawk. |
| mawk | Fast awk; extension set differs; awkrs accepts some `-W` tokens for CLI compatibility only. |
| gawk | Highest overlap; awkrs implements many gawk builtins and globals directly or as Rust builtins (see `src/gawk_extensions.rs`). |
| `@load` | awkrs inlines **`.awk`** sources or maps known gawk module names; **does not** load arbitrary `.so` extensions (`src/source_expand.rs`). |
| Parallel records (`-j`) | **awkrs-only** execution path when the program is parallel-safe (`parallel::record_rules_parallel_safe`); can diverge from any sequential reference. |

---

## 2. Command-line interface

| Flag / option | POSIX awk | BSD awk | mawk | gawk | awkrs |
|---------------|-----------|---------|------|------|-------|
| `-f` program file | Yes | Yes | Yes | Yes | **Match** |
| `-F` FS | Yes | Yes | Yes | Yes | **Match** |
| `-v var=val` | Yes | Yes | Yes | Yes | **Match** |
| Program + file operands | Yes | Yes | Yes | Yes | **Match** |
| `-e` / `-i` | — | — | **Part** | Yes | **Match** (multiple `-e`/`-i`) |
| `-b` characters-as-bytes | — | — | — | Yes | **Part** (wired into runtime; verify vs release I/O paths) |
| `-c` traditional | — | — | — | Yes | **Part** (reserved; stricter rules incremental) |
| `-C` copyright | — | — | — | Yes | **Match** (prints the awkrs copyright line and exits) |
| `-d` dump-variables | — | — | — | Yes | **Part** (dump after run; format awkrs-specific) |
| `-D` debug | — | — | — | Yes | **Part** (listing/dump; not gawk’s debugger) |
| `-E` exec | — | — | — | Yes | **Match** (program from FILE; remaining args are data) |
| `-g` gen-pot | — | — | — | Yes | **Match** (awkrs POT generator) |
| `-I` trace | — | — | — | Yes | **No** (parsed for CLI compatibility; no runtime effect — `Args::trace` is never read outside `src/cli.rs`) |
| `-k` / `--csv` | — | — | — | Yes | **Match** (CSV / `FPAT` mode per `Runtime::csv_mode`) |
| `-l` load / `AWKPATH` | — | — | — | Yes | **Part** (library search; no dynamic `.so`) |
| `-L` lint | — | — | — | Yes | **Part** (`lint_warn` / fatal modes) |
| `-M` bignum | — | — | — | Yes | **Part** (MPFR path; `PROCINFO["prec"]` / `roundmode`) |
| `-N` use-lc-numeric | — | — | — | Yes | **Match** (formatting path; string→number still `.` per `cli.rs` docs) |
| `-n` non-decimal-data | — | — | — | Yes | **Match** (`set_numeric_parse_mode`) |
| `-o` pretty-print | — | — | — | Yes | **Part** (AST listing; not gawk’s `--pretty-print` text) |
| `-O` optimize | — | — | — | Yes | **Match** (accepted; JIT on unless `-s`) |
| `-p` profile | — | — | — | Yes | **Part** (awkrs wall-clock summary; not gawk profiler format) |
| `-P` posix | — | — | — | Yes | **Part** (runtime flag; incremental strictness) |
| `-r` re-interval | — | — | — | Yes | **Match** (no-op; intervals always on) |
| `-s` no-optimize | — | — | — | Yes | **Match** (disables JIT) |
| `-S` sandbox | — | — | — | Yes | **Part** (`require_unsandboxed_io`; `system()` blocked, etc.) |
| `-t` lint-old | — | — | — | Yes | **Part** |
| `-W opt` (mawk) | — | — | Yes | — | **Part** (`help`/`version`/`exec=` merged; other tokens ignored) |
| `-j` / `--threads` | — | — | — | — | **Ext** (awkrs parallel pool) |
| `--read-ahead` | — | — | — | — | **Ext** (stdin chunking with `-j`) |
| `--repl` | — | — | — | — | **Ext** (reedline REPL; also the default on a bare tty) |
| `--lsp` | — | — | — | — | **Ext** (Language Server over stdio) |
| `--dap [HOST:PORT]` | — | — | — | — | **Ext** (Debug Adapter over stdio or TCP) |
| `--aot OUT` | — | — | — | — | **Ext** (AOT-compile a `BEGIN`-only program to a native executable) |
| `--dump-tokens` / `--dump-ast` / `--dump-bytecode` / `--disasm` | — | — | — | — | **Ext** (compiler introspection; each prints and exits) |
| `--tiers` | — | — | — | — | **Ext** (reports which fusevm execution tier took each chunk) |

---

## 3. Source directives and namespaces

| Feature | BSD | mawk | gawk | awkrs |
|---------|-----|------|------|-------|
| `@include "file"` | No | No | Yes | **Match** (pre-parse expand) |
| `@load "x.awk"` / bundled names | No | No | Yes | **Part** (`.awk` inline only; no `.so`) |
| `@namespace "ns"` | No | No | Yes | **Match** (`apply_default_namespace`) |
| `ns::name` identifiers | No | No | Yes | **Match** (`lexer` / namespace pass) |

---

## 4. Language constructs (selected)

| Construct | BSD | mawk | gawk | awkrs |
|-----------|-----|------|------|-------|
| `BEGIN` / `END` | Yes | Yes | Yes | Yes | **Match** |
| `BEGINFILE` / `ENDFILE` | No | No | No | Yes (Ext) | **Match** (gawk-style; `next`/`nextfile` invalid in `BEGINFILE` per `vm.rs`) |
| Range patterns (`pat1,pat2`) | Yes | Yes | Yes | **Match** |
| Regex record patterns + compound (`/re/ && expr`) | Yes | Yes | Yes | **Match** (tests in `tests/extra_integration.rs`) |
| `next` / `nextfile` / `exit` | Yes | Yes | Yes | **Match** |
| User functions / `return` | Yes | Yes | Yes | **Match** |
| `delete a[k]` / `delete a` | Yes | Yes | Yes | **Match** |
| `for (i in a)` order | Unspecified | Unspecified | gawk sorts / `sorted_in` | **Part** (hash order vs `PROCINFO["sorted_in"]`; `-P` skips gawk ordering) |
| `switch` | No | No | Yes | Yes | **Match** |
| Indirect function call (`@` / function pointer) | No | No | Yes | Yes | **Part** (see `Expr::IndirectCall`; edge cases vs gawk) |
| Coprocess (`\|&`) | No | No | Yes | **Part** (runtime has coproc types; parity not guaranteed) |
| `getline` variants | Yes | Yes | Yes | **Part** (incl. `PROCINFO` timeout/retry — see `runtime.rs`) |

---

## 5. Special variables

| Variable | BSD | mawk | gawk | awkrs |
|----------|-----|------|------|-------|
| `NR` `FNR` `NF` `$0` `$n` | Yes | Yes | Yes | **Match** (invalid `NF` / negative fields fatal like gawk — tested) |
| `FS` `RS` `OFS` `ORS` `OFMT` `CONVFMT` | Yes | Yes | Yes | **Match** — including streaming multi-char `RS`, regex `RS`, and paragraph mode (`RS == ""`) over both stdin and files (trailing-newline trim matches gawk). |
| `FILENAME` `ARGC` `ARGV` `ENVIRON` | Yes | Yes | Yes | **Match** |
| `SUBSEP` | Yes | Yes | Yes | **Match** |
| `RSTART` `RLENGTH` | Yes | Yes | Yes | **Match** |
| `RT` | No | Part | Yes | **Match** |
| `ARGIND` | No | No | Yes | **Match** |
| `ERRNO` | No | No | Yes | **Match** |
| `PROCINFO` | No | No | Yes | **Part** (keys: `sorted_in`, read timeout, errno, FS mode, bignum, identifiers, etc. — not every gawk key) |
| `SYMTAB` `FUNCTAB` | No | No | Yes | **Part** (reflection best-effort) |
| `FIELDWIDTHS` `FPAT` | Part | Part | Yes | **Match** — `FIELDWIDTHS` accepts gawk's `width`, `skip:width`, and `*` tokens; the last entry is clamped to its declared width (no auto-extend), so any trailing input bytes are left unused like gawk. |
| `IGNORECASE` | Part | Part | Yes | **Match** — applies to multi-char regex FS, `match`/`sub`/`gsub`/`split`/`gensub`, and `~`/`!~`. Single-char string FS (and single-char `split` separator) is always literal, independent of `IGNORECASE` (gawk parity). |
| `BINMODE` | No | No | Yes | **Part** |
| `LINT` | No | No | Yes | **Part** |
| `TEXTDOMAIN` | No | No | Yes | **Part** (gettext path) |

---

## 6. Built-in functions

Columns: **P** = POSIX / universal core, **B** = BSD awk, **M** = mawk, **G** = gawk extension (approximate; BSD may add some).

| Builtin | P | B | M | G | awkrs |
|---------|---|---|---|---|--------|
| `atan2` `cos` `sin` `exp` `log` `sqrt` `int` | * | * | * | * | **Match** (negative `log`/`sqrt`: warn + NaN like gawk — `runtime::warn_builtin_negative_arg`) |
| `rand` `srand` | * | * | * | * | **Part** (sequence not guaranteed to match any one engine) |
| `length` / `length()` | * | * | * | * | **Match** (bare `length` → `$0` — `parser.rs`) |
| `index` `substr` `sprintf` | * | * | * | * | **Match** |
| `match` `sub` `gsub` `split` | * | * | * | * | **Match** / **Part** (regex engine = Rust `regex`; subtle differences possible). `gsub(//, …)` produces gawk's zero-width matches at every position; `split(s, a, fs, seps)` populates the 4th-arg `seps` array with the actual separator strings between fields. |
| `tolower` `toupper` | * | * | * | * | **Match** |
| `system` `close` | * | * | * | * | **Match** — `system()` flushes buffered stdout / pipes / files before invoking the subprocess; `close()` returns -1 for an unopened name and the exit code / 0 for a clean close (gawk parity, `runtime::close_handle`) |
| `strtonum` | *¹ | Part | Part | Yes | **Match** |
| `asort` `asorti` | — | — | — | Yes | **Match** |
| `gensub` `patsplit` | — | — | — | Yes | **Part** |
| `mktime` `strftime` `systime` `gettimeofday` | — | — | Part | Yes | **Part** |
| `and` `or` `xor` `compl` `lshift` `rshift` | — | — | — | Yes | **Match** |
| `isarray` `typeof` `mkbool` | — | — | — | Yes | **Match** / **Part** |
| `intdiv` `intdiv0` | — | — | — | Yes | **Match** |
| `bindtextdomain` `dcgettext` `dcngettext` | — | — | — | Yes | **Part** (`gettext_util` / stubs) |
| `chdir` `stat` `statvfs` `fts` | — | — | — | Ext / Yes | **Match** / **Part** (`gawk_extensions.rs`) |
| `readfile` `ord` `chr` `sleep` | — | — | — | Ext | **Match** (as builtins) |
| `revoutput` `revtwoway` `rename` | — | — | — | Ext | **Match** |
| `inplace_tmpfile` `inplace_commit` | — | — | — | Ext | **Match** |
| `writea` `reada` | — | — | — | Ext | **Match** |
| `intercept` `intercept_proceed` `intercept_list` `intercept_remove` `intercept_clear` | — | — | — | — | **awkrs-only** — aspect-oriented before/after/around advice on user-function calls (ported from `zshrs`; no POSIX/gawk counterpart). See §0x03 of the README. |

¹ `strtonum` appears in POSIX awk revision used by gawk; older texts omit it.

---

## 7. `printf` / `print` / numeric formatting

| Topic | awkrs |
|-------|--------|
| `%g` / `%G` | **Match** — precision is total significant digits (C99/POSIX); the fixed-vs-`e` form decision uses the **rounded** exponent (so `%.1g` of `9.5` is `1e+01`, not `10`). Precision 0 is treated as 1. |
| `%u` on negative values | **Match** — wraps via i64→u64 two's complement (gawk parity), not clamped to 0. |
| `0` flag on `%s` / `%c` | **Match** — POSIX says the flag is for numeric conversions only; awkrs pads with spaces for string/char conversions. |
| Unknown conversion letters (`%q`, `%v`, …) | **Match** — emit the literal `%X` without consuming an argument (gawk parity). |
| `%a` / `%A` hex float | **Match** (`format_hex_float` in `src/format.rs`; gawk parity confirmed) |
| Non-finite floats (`±inf`, `±nan`) across `%f`/`%e`/`%g`/`%a` | **Match** (gawk-style `+inf` / `-inf` / `+nan` / `-nan`, with `INF` / `NAN` for uppercase variants — `format_non_finite` in `src/format.rs`) |
| `print` of non-finite values | **Match** — `format_number` in `src/runtime.rs` emits the same `+inf` / `+nan` spelling so `print x` and `printf "%s", x` agree |
| `LC_NUMERIC` (`-N`) | **Part** (documented split: format vs parse) |
| `%'` flag thousands grouping | **Match** — consults `localeconv()->thousands_sep` regardless of `-N` (gawk parity). Empty in `LC_ALL=C` → no grouping; `","` in `en_US.UTF-8` → comma grouping. |
| `==` / `<` / `>` of `Num` vs string literal | **Match** — string-compare fallback stringifies the number via `CONVFMT` (not the default `%.6g`). E.g. `BEGIN{CONVFMT="%.2f"; print 3.14159=="3.14"}` prints `1`. |
| `a % 0` / `a %= 0` | **Match** — fatal "division by zero attempted in `%'" (was previously NaN). |
| Numeric coercion of `"inf"` / `"nan"` | **Match** — bare special names coerce to 0; only signed three-letter `inf` / `nan` (case-insensitive) are accepted. `"+infinity"` is rejected like in gawk. |
| `lshift` / `rshift` / `compl` negative args | **Match** — fatal "negative values are not allowed". |
| `typeof($field)` of noisy numeric text (e.g. `"42abc"`) | **Match** — reports `"string"` (numeric prefix alone is not enough); field comparisons against numbers use string-compare. Pure-numeric text (`"42"`) still reports `"strnum"`. |
| `match(str, re, arr)` start/length subscripts | **Match** — writes `arr[i, "start"]` (1-based char index) and `arr[i, "length"]` for each successful submatch; unmatched optional groups have NO entries. |
| `mktime(spec [, utc])` | **Match** — optional second argument forces UTC interpretation when truthy; one-arg form remains local-time. |
| Assignment in ternary else-branch (`1 ? x=1 : x=2`) | **Match** — the else-branch parses as an assignment-expression (gawk grammar). Previously rejected as "invalid assignment target". |
| `asort` / `asorti` on unassigned name | **Match** — treats missing slot as an empty array (returns 0). Scalar values still raise the "first argument is not an array" fatal. Compiler tracks these positions for array-slot promotion. |
| Numeric `==` precision | **Match** — bit-exact (POSIX). Previously used a fuzzy `f64::EPSILON` tolerance, so `0.1 + 0.2 == 0.3` returned true (the difference is ~5.55e-17, below EPSILON). Now matches gawk's 0. |
| Paragraph-mode `RT` (`RS == ""`) | **Match** — captures the FULL run of trailing newlines from the last content line plus the blank lines separating records (`b\n\nc` → RT == "\n\n"). The last record also captures EOF-trailing newlines into RT. |
| `PROCINFO["strftime"]` default | **Match** — `"%a %b %e %H:%M:%S %Z %Y"` (gawk's date(1)-equivalent default), not `"%c"`. |
| `printf("fmt", a, b)` function-call form | **Match** — equivalent to `printf "fmt", a, b`. Previously rejected as "parenthesized comma list is not allowed". Mixed paren-args + bare args (`printf(a,b), c`) still rejected. |
| Builtin called with wrong arity | **Match (no panic)** — uniform `"N is invalid as number of arguments for X"` error across `tolower`, `toupper`, `index`, `substr`, `length`, `system`, `close`, `rand`, `srand`, `asort`, `asorti`, `split`, `match`, `sub`, `gsub`, `exp`, `log`, `sin`, `cos`, `sqrt`, `atan2`, `int`. Earlier awkrs panicked on some, silently ignored extras on others, and used a non-gawk wording on the math functions. |
| `delete x` / `delete x[k]` on a scalar | **Match** — fatal "attempt to use scalar `x' as an array". Unassigned names still silently no-op (POSIX). |
| What counts as a POSIX **numeric string** | **Match** — only input-derived values (fields, `getline` targets, `split` elements, `ARGV`/`ENVIRON`) are strnum. A *computed* string never is, so `substr("065",1,2) == 6`, `sprintf("%s","06") == 6`, `toupper("06") == 6` and `$1 "" == 6` are all string compares and answer 0. awkrs previously carried the strnum-capable `Value::Str` out of `substr`/`sprintf`/`toupper`/`tolower`/`gensub`/`strftime` and out of concatenation, and answered 1. `typeof` reports `"strnum"` on the same rule the comparisons use. |
| Empty record field count | **Match** — an empty record has `NF == 0` under every `FS`. The single-char and regex splitters previously pushed one empty range and reported `NF == 1` for a blank line under `FS=":"`. |
| `sub` / `gsub` that matches nothing | **Match** — the target is left completely untouched, so an uninitialized variable stays uninitialized (`sub(/x/,"y",z); z == 0` is still 1) and a number stays a number. The unchanged string used to be written back, demoting strnum to string. |
| Bare `return` (and falling off the end of a function) | **Match** — yields the uninitialized value, equal to both `0` and `""`. |
| `split(s, a, /re/)` with a **regex literal** separator | **Match** — always a regex, so the `FS` shorthands never apply: `/ /` is one literal space (`split("  a  b  ", A, / /)` is 7, not 2) and `/./` is any-character. An empty separator (`//` or `""`) still splits into characters. |
| Multidimensional subscripts and `CONVFMT` | **Match** — each subscript converts like a single subscript: integral values exactly, everything else through `CONVFMT`. `CONVFMT="%.2f"; A[1.234,2]` keys on `1.23<SUBSEP>2`. |
| `CONVFMT` subscript in **every** subscript operation | **Match** — the `CONVFMT` rendering is the array's identity, so `k in a`, `delete a[k]`, `a[k] op= v`, `a[k]++`/`--` and `typeof(a[k])` all key exactly as the `a[k] = …` that created the entry. `CONVFMT="%.2f"; x=1.23456; A[x]=5; A[x]+=1` leaves **one** entry `A["1.23"]` of `6`. awkrs previously converted the key differently in those five operations, so `x in A` was false and the compound assignment created a second entry under the full-precision spelling. |
| `CONVFMT` in **every string builtin** | **Match** — POSIX gives one rule for turning a number into a string outside `print`, and `length`, `substr`, `index` (both operands), `toupper`, `tolower`, `split`'s subject, `sub`/`gsub`'s target and replacement, and `gensub`'s subject all follow it. `CONVFMT="%.2f"; x=1.23456` gives `length(x)==4`, `substr(x,3)=="23"`, `index("a1.23b",x)==2` and leaves `gsub(/3/,"9",x)` as `1.29`. awkrs previously read the number at full `f64` precision in all of them (`length(x)` was 7, `gsub` left `1.29456`), so `CONVFMT` was honoured by concatenation, comparison and subscripts but ignored one call away. |
| `CONVFMT` for a **dynamic regex** | **Match** — a dynamic regex is the string value of its operand, so a numeric pattern converts the same way: `CONVFMT="%.2f"; x=1.23456; "a1.23b" ~ x` is true, and `match`, `split`'s separator, `sub`/`gsub`'s pattern and `patsplit` agree. Only the *subject* side of `~`/`!~` used to convert this way, so `"a1.23b" ~ x` was false while `"a1.23b" == x` — the same coercion one operator apart — was true. |
| `CONVFMT` for a `getline` redirect target | **Match** — `getline … < expr` and `expr \| getline` name a file or command as a string, so a numeric operand opens the `CONVFMT` rendering. awkrs previously looked for the full-precision spelling and returned −1. |
| When the `CONVFMT` coercion is performed | **Match** — at the point of **use**, never cached at assignment: `CONVFMT="%.2f"; x=1.23456; a=length(x); CONVFMT="%.4f"; b=length(x)` yields `4 6` in all three references. Integral values bypass the format entirely, and an input-derived value keeps its original text (`$1` of the record `1.23456` is still 7 characters under `"%.2f"`) — only computed `Num`/`Mpfr` values are rendered. |
| `printf "%c"` of a numeric string | **Match** — a strnum operand is numeric, so `echo 65 \| awk '{printf "%c", $1}'` prints `A`, while the string literal `"65"` prints `6`. |
| `printf` negative `*` precision | **Match** — ISO C: a negative precision argument is taken as if the precision were omitted, so `printf "%.*f", -2, 3.14159` prints `3.141590`. awkrs previously clamped it to 0. |
| `;` as a control-flow body | **Match** — POSIX makes `;` a statement, so `if (c) ;`, `while (c) ;`, `for (…) ;` and `else ;` all parse. awkrs previously rejected every one of them. |
| `split(s, a)` on an empty string | **Match** — the target becomes an (empty) array, so `typeof` reports `"array"` rather than `"untyped"`. |
| Scalar used as array (`x[k]=…`, `x[k]`, `k in x`, `for (k in x)`) | **Match** — fatal "attempt to use scalar `x' as an array". Earlier awkrs silently auto-promoted on write, returned empty on read, returned 0 from `in`, and ran zero iterations on for-in. |
| `printf "%u"` of values past 2^64 | **Match** — falls back to `%g`-style formatting (`2^65` → `"3.68935e+19"`). The exact 2^64 boundary still prints as the u64::MAX digit string. Earlier awkrs saturated all over-u64 values at u64::MAX. |
| MPFR (`-M`) | **Part** (precision / rounding via `PROCINFO`) |

---

## 8. Regular expressions

| Topic | awkrs |
|-------|--------|
| Engine | Rust `regex` crate (not literal GNU regex copy). |
| Interval quantifiers `{m,n}` | Enabled ( `-r` is no-op). |
| `IGNORECASE` | Supported for split/match contexts that consult runtime. |
| `.` matches `\n` | **Match** — all built regexes use `dot_matches_new_line(true)` (gawk ERE convention). |
| Backreferences in patterns (e.g. `(.)\1`) | **No** — Rust regex is linear-time and does not support pattern-side backrefs. (Backrefs in **replacement** text via `gensub` `\1`..`\9` and `&` are supported.) |
| POSIX character classes (`[[:digit:]]`, etc.) | **Match** |
| NUL bytes / binary | **Part** (`-b` / `BINMODE` — exercise before relying on). |

---

## 9. Known intentional or unavoidable divergences

- **JIT** (fusevm's Cranelift, via `src/fusevm_bridge.rs`): When enabled, must match interpreter; if a mismatch is found, treat as a bug in JIT, not as "gawk is wrong." Eligibility is an allowlist of numeric ops (`is_fusevm_eligible`); AWK-specific ops including `~`/`!~` regex match lower to `fusevm::Op::Extended` and run on the interpreter, not the JIT.
- **Parallel mode** (`-j`): Record rules may run concurrently; programs with side effects or dependence on global order are unsafe.
- **Dynamic extensions**: gawk `@load "foo.so"` has no equivalent in awkrs.
- **Process / locale / OS**: `PROCINFO["platform"]` mapping uses `posix`/`mingw` style (`procinfo.rs`), not necessarily gawk’s host string for every OS.
- **For-in order**: Without `-P`, gawk-style `sorted_in` and user comparators apply; hash order still differs across engines when sorting is off.
- **Exit status**: fatal conditions (runtime faults, an unreadable `-f` file, an input file that cannot be opened, output-redirection I/O errors) exit **2**, matching all three reference awks. Parse diagnostics exit **1**, matching gawk; mawk and one-true-awk exit 2 there. See `Error::exit_status` in `src/error.rs`.
- **`printf "%c"` with an empty string**: emits nothing, matching POSIX ("the first character of the string value") and one-true-awk. gawk and mawk emit a NUL byte.
- **`"0x10" + 0`**: `0`, matching POSIX, gawk and mawk. one-true-awk's `strtod` accepts the `0x` prefix and yields 16.
- **`printf` unsigned conversions of a negative argument** (`%x` `%o` `%X`): converted as a 64-bit unsigned value (`printf "%x", -3` → `fffffffffffffffd`), matching gawk. one-true-awk and mawk both print `0`.
- **`OFMT` / `CONVFMT` set to a non-floating-point conversion** (e.g. `"%d"`): undefined by POSIX, and all three references differ — one-true-awk ignores the setting, mawk prints a garbage integer, gawk warns and prints `0`. awkrs produces gawk's value without the warning.
- **Paragraph mode field splitting**: with `RS == ""` a single-character `FS` gains `<newline>` as an additional separator (gawk and one-true-awk both do this; mawk does not). A regex `FS` is left alone in every reference, so an embedded newline stays inside the field.
- **Character semantics are UTF-8, not locale-driven**: `length`/`substr`/`index`/`toupper`/`tolower` — and `match`'s `RSTART`/`RLENGTH`, which report 1-based **character** positions in the same unit — count and fold Unicode scalar values regardless of `LC_ALL`, so `length("é")` is 1 even under `LC_ALL=C` where gawk reports 2. `-b` selects byte semantics explicitly, and it now governs **case folding as well as counting**: under `-b`, `toupper`/`tolower` fold ASCII only, so `toupper("café")` is `CAFé` and `toupper("Straße")` is `STRAßE` — reproducing all three references in the C locale, and keeping the result the same length as its input where Unicode's `ß` → `SS` would grow it. `-b` previously switched the counting builtins but left folding Unicode-aware, so a single `-b` run reported `length("café") == 5` while `toupper("café")` returned `CAFÉ` — the byte world and the character world in one program. `printf "%c"` of a code point above 255 follows the default rule: awkrs emits the UTF-8 encoding (gawk's behaviour in a UTF-8 locale), where gawk and mawk under `LC_ALL=C` truncate to a byte and one-true-awk emits nothing.
- **A function name used as a variable** (`function f(){} BEGIN{ f = 1 }`) is rejected before the program runs, matching all three references. The **status** is a three-way split: gawk exits 1 (it diagnoses at parse time), mawk and one-true-awk exit 2. awkrs exits **2** with the other two, because the check runs in `validate_program` after parsing rather than inside the grammar. A function *parameter* that shadows a function name is legal everywhere and stays legal here.
- **`close()` of a pipe** returns the command's exit status, matching gawk and mawk; one-true-awk returns 0.
- **`getline < <directory>`** reads the directory's entries, one file name per record, sorted. This is an awkrs extension (the same data `readdir()` returns); gawk and mawk return −1 for a directory and one-true-awk reports an I/O error. A script that means to read a file and is handed a directory therefore sees records here where the references see a failure.
- **Record splitting reads a numeric `FS` / `RS` without `CONVFMT`** — the one part of the `CONVFMT`-coercion rule above that is still open, and it is deliberately partial. `BEGIN { CONVFMT="%.2f"; FS=1.23456 }` splits *records* on the full-precision `1.23456` where all three references split on `1.23`; `RS` behaves the same way, and `OFS` / `ORS` match mawk rather than gawk and one-true-awk. The **explicit** separator forms are all correct — `split(s, a, fs)`, and also the two-argument `split(s, a)` that falls back to `FS` — so within awkrs the same `FS` value can separate a `split()` call and a record differently. That inconsistency is the smaller of the two available ones: reading `FS` is not a single site (the record splitter, the `$0`-assignment path, the field-rebuild path and the JIT host each read it independently, and the splitter caches the value per record), so converting at only some of them would put the interpreter and the JIT tier into disagreement, which §9 treats as a bug in its own right. Converting at *assignment* is not the answer either: `print FS` uses `OFMT` in every reference (`CONVFMT="%.2f"; OFMT="%.3f"; FS=1.23456; print FS` prints `1.235`), so the variable has to keep its numeric identity. `length(FS)` and the other string builtins are already correct, because those go through the coercion above. Repro: `printf 'a1.23b\n' | awk 'BEGIN{CONVFMT="%.2f"; FS=1.23456}{print NF}'` — `2` in gawk/mawk/one-true-awk, `1` here.
- **`typeof` of a never-assigned function parameter**: gawk turns a parameter from `"untyped"` into `"unassigned"` the first time it is read; awkrs reports `"untyped"` throughout. Global scalars and array elements do make that transition (a per-slot "touched" bit in `Runtime::slot_touched`); function locals live in a per-call frame map that has nowhere to record it, and adding a parallel per-frame structure would cost work on every user-function call for one value of one gawk-only introspection builtin.
- **`typeof($0)` before any record is read**: gawk reports `"unassigned"`, awkrs reports `"string"` — `$0` starts as an empty record rather than a distinct never-assigned state.

---

## 10. How to extend this matrix

1. Add a row when a user reports a behavioral delta; cite **minimal** repro and which engine defines the expected result.
2. Prefer a **regression test** under `tests/` over a permanent “known bug” row.
3. Update **`BUILTIN_NAMES`** (`src/namespace.rs`) / `exec_builtin_dispatch` (`src/vm_builtins.rs`) when adding builtins, then mirror here.

---

*Generated from source audit; not a legal conformance statement.*
