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
| `-f` program file | Yes | Yes | Yes | Yes | **Match** — an *empty* program file is a program with no rules: it runs, reads nothing and exits 0. awkrs decided whether a program had been supplied from the assembled program bytes, so `awk -f empty.awk data.txt` took `data.txt` as the program text and died on a parse error, while `awk -f empty.awk` alone reported "no program given". An empty program on `argv` (`awk '' data.txt`) already worked. |
| `-F` FS | Yes | Yes | Yes | Yes | **Match** — POSIX defines `-F sepstring` as the assignment `FS=sepstring`, so the value gets the same escape processing as `-v`: `-F '\t'` is a one-character tab and `-F '\\.'` the two characters `\.` (a regex for a literal dot). awkrs stored the argument verbatim, so `length(FS)` was 2 for the tab and backslashes reached the regex compiler undecoded, while the identical value written `-v FS='\t'` came out right. |
| `-v var=val` | Yes | Yes | Yes | Yes | **Match** — the value is processed as a string literal, so `-v 's=a\tb\n'` is 4 characters. awkrs stored the raw argument and answered 6; it now runs the value through the lexer's own escape table (`lexer::unescape_assignment_value`) rather than a second copy of the rules. The name has to be an awk identifier, optionally qualified as `namespace::name`; anything else is fatal (`-v 1x=3` → "`1x' is not a legal variable name", exit 2), which is what all three references do. awkrs accepted every spelling and created a variable under a name no program can write, so a mistyped flag ran silently with the variable unset. |
| Program + file operands | Yes | Yes | Yes | Yes | **Match** — the operand `-` names standard input and reports `FILENAME` as `-`, so `awk '…' data.txt - < more.txt` appends a pipe to a file list. awkrs opened a file literally named `-` and died with a fatal "cannot open file"; `getline < "-"` was already redirected but the operand was not. |
| `var=value` operand (assignment between files) | Yes | Yes | Yes | Yes | **Match** — an operand whose left side is a valid identifier assigns instead of naming a file, takes effect at the position it occupies, is a POSIX *numeric string*, and gets the same escape processing as `-v`. When every operand is an assignment the program still reads standard input. awkrs read them as file names and failed with `cannot open file "v=1"`. A namespace-qualified name is an assignment too (`ns::x=1`, gawk only), and gawk's `awk::` prefix names the global variable wherever it appears — `awk::x=1` as an operand, `-v awk::x=1`, and `awk::x` or `awk::f()` in the program, including inside an `@namespace`; awkrs used to read `ns::x=1` as a file name and treat `awk::x` as a separate variable. |
| `-e` / `-i` | — | — | **Part** | Yes | **Match** (multiple `-e`/`-i`) |
| `-b` characters-as-bytes | — | — | — | Yes | **Part** (wired into runtime; verify vs release I/O paths) |
| `-c` traditional | — | — | — | Yes | **Part** (gawk-extension builtins refused; BSD `%0Ns`/`%0Nc` zero padding; other gawk `--traditional` restrictions not applied — `BEGINFILE`/`ENDFILE` still run) |
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
| `BEGINFILE` / `ENDFILE` | No | No | No | Yes (Ext) | **Match** (gawk-style; `next`/`nextfile` invalid in `BEGINFILE` per `vm.rs`). Every `BEGINFILE` starts from an empty record (`$0` is `""`, `NF` is 0), as in gawk; awkrs used to show the previous file's last record there. |
| Range patterns (`pat1,pat2`) | Yes | Yes | Yes | **Match** |
| Regex record patterns + compound (`/re/ && expr`) | Yes | Yes | Yes | **Match** (tests in `tests/extra_integration.rs`) |
| `next` / `nextfile` / `exit` | Yes | Yes | Yes | **Match** |
| User functions / `return` | Yes | Yes | Yes | **Match** |
| `delete a[k]` / `delete a` | Yes | Yes | Yes | **Match** |
| `for (i in a)` order | Unspecified | Unspecified | gawk sorts / `sorted_in` | **Part** (hash order vs `PROCINFO["sorted_in"]`; `-P` skips gawk ordering). The `@ind_*`/`@val_*` orders are gawk's `sort_up_*` comparisons (array.c): every one breaks a tie on the index string compared bytewise, a descending mode is the exact reverse of the ascending one (ties included), equal numeric indices (`10`, `010`, `1e1`) fall back to the index string, and an unassigned element or a subarray sends `@val_str`/`@val_num` to the `@val_type` order, which puts unassigned elements below scalars and subarrays above them. Value strings compare by bytes, not `strcoll`. The order applies to a function's array parameter and to a local array as well as to a global; awkrs used to leave ties in hash order and sort only global arrays. |
| `switch` | No | No | Yes | Yes | **Match** |
| Indirect function call (`@` / function pointer) | No | No | Yes | Yes | **Part** (see `Expr::IndirectCall`; edge cases vs gawk) |
| Coprocess (`\|&`) | No | No | Yes | **Part** — `print \|& cmd`, `cmd \|& getline [var]` and `getline [var] <& cmd` share one two-way process, and `close(cmd, "to")` closes only its input so a filter such as `sort` sees EOF while its output stays readable. awkrs used to parse `cmd \|& getline` as a one-way `\|`, which started a second process reading awk's own stdin. No pty mode (`PROCINFO[cmd, "pty"]`). |
| `getline` variants | Yes | Yes | Yes | **Part** (incl. `PROCINFO` timeout/retry — see `runtime.rs`). Plain `getline` continues into the next input operand at end of file (applying `var=value` operands on the way, setting `FILENAME`/`FNR`), and in `BEGIN` opens the first operand; awkrs used to stop at the first file's end and leave `FILENAME` empty in `BEGIN`. When `getline` reaches the end of a file it runs `ENDFILE` for it and, moving on, `BEGINFILE` for the next (from `BEGIN`, `BEGINFILE` for the first file or standard input it opens), and the record loop does not run them a second time — gawk's order; awkrs used to run neither, and ran `ENDFILE` once for the last file at the end. `cmd | getline [var]` binds tighter than comparison and assignment, as in all three references: `while ("cmd" | getline line > 0)` compares getline's result, and `r = "cmd" | getline x` assigns it (the first used to be a parse error, the second piped the assignment). |

---

## 5. Special variables

| Variable | BSD | mawk | gawk | awkrs |
|----------|-----|------|------|-------|
| `NR` `FNR` `NF` `$0` `$n` | Yes | Yes | Yes | **Match** (invalid `NF` / negative fields fatal like gawk — tested) |
| `FS` `RS` `OFS` `ORS` `OFMT` `CONVFMT` | Yes | Yes | Yes | **Match** — including streaming multi-char `RS`, regex `RS`, and paragraph mode (`RS == ""`) over both stdin and files (trailing-newline trim matches gawk). |
| `FILENAME` `ARGC` `ARGV` `ENVIRON` | Yes | Yes | Yes | **Match** — `ARGV` is consulted as awk walks the operands, not snapshotted at startup, so `BEGIN { delete ARGV[1] }` skips that file, setting an element to `""` skips it, and rewriting one redirects the read. awkrs iterated its own argv and read a deleted file anyway. |
| `SUBSEP` | Yes | Yes | Yes | **Match** |
| Arrays of arrays (`a[i][j]`) | No | No | Yes | **Match** — an element can hold a subarray, to any depth: `a[i][j] = v` and a read of `a[i][j]` create the subarray `a[i]` when it is missing, `k in a[i]`, `for (k in a[i])` (in `PROCINFO["sorted_in"]` order), `delete a[i][j]`, `++`/`--` and `op=` at depth, `length`/`isarray`/`typeof` of a subarray, a subarray (or a missing element, which the callee may turn into one) passed by reference to a user function, `SUBSEP` keys inside a path (`a[i, j][k]`), and `split`/`patsplit`/`asort`/`asorti`/`match` filling, or `sub`/`gsub` editing, a subarray element. gawk's fatals hold: a subarray read or overwritten as a scalar is `` attempt to use array `a["1"]' in a scalar context ``, and indexing a scalar element (even one a read left unassigned) is `` attempt to use scalar `a["1"]["2"]' as an array ``. awkrs used to reject `a[i][j]` as a parse error. The one difference: gawk reports an element created by `length(a[k])`, `isarray(a[k])` or a function argument as `untyped` and one created by a plain read as `unassigned`; awkrs reports both as `unassigned`, and `typeof(a[k])` of a missing element does not create it as gawk's does. |
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
| `rand` `srand` | * | * | * | * | **Match** gawk — `rand()` is a port of gawk's generator (its bundled BSD `random(3)`, `TYPE_4` with the 512-entry shuffle, behind `do_rand`), so the sequence from any `srand(n)`, and the unseeded one, is gawk's value for value; mawk and one-true-awk each have their own. `srand` returns the previous seed and `srand()` seeds with the time in whole seconds. awkrs used a 15-bit LCG. |
| `length` / `length()` | * | * | * | * | **Match** (bare `length` → `$0` — `parser.rs`) |
| `index` `substr` `sprintf` | * | * | * | * | **Match** |
| `match` `sub` `gsub` `split` | * | * | * | * | **Match** / **Part** (regex engine = Rust `regex`; subtle differences possible). `gsub(//, …)` produces gawk's zero-width matches at every position; `split(s, a, fs, seps)` populates the 4th-arg `seps` array with the actual separator strings between fields; with the default `" "` separator it also puts leading whitespace in `seps[0]` and trailing whitespace in `seps[n]` (each only when present), as gawk does. `patsplit`'s `seps[0]` / `seps[n]` likewise hold the text before the first and after the last field. |
| `tolower` `toupper` | * | * | * | * | **Match** |
| `system` `close` | * | * | * | * | **Match** — `system()` flushes buffered stdout / pipes / files before invoking the subprocess; `close()` returns -1 for an unopened name and the exit code / 0 for a clean close (gawk parity, `runtime::close_handle`). A child killed by a signal reports `256 + signo` from **both** (`system("kill -TERM $$")` → 271), the encoding all three references use; it lived only in `close_handle` and `system()` answered -1, so both now share `runtime::awk_process_status`. |
| `strtonum` | *¹ | Part | Part | Yes | **Match** |
| `asort` `asorti` | — | — | — | Yes | **Match** — including the third argument: an `@ind_*` / `@val_*` ordering or a `(i1, v1, i2, v2)` comparison function. |
| `gensub` `patsplit` | — | — | — | Yes | **Part** |
| `mktime` `strftime` `systime` `gettimeofday` | — | — | Part | Yes | **Part** |
| `and` `or` `xor` `compl` `lshift` `rshift` | — | — | — | Yes | **Match** — every one computes in 64-bit unsigned arithmetic on `(uintmax_t)`-cast operands and narrows the result to a double's 53 bits like gawk's `adjust_uint`: `compl(0)` = 9007199254740991 (not -1), `or(2^54, 1)` = 1, and a shift count of 64 or more yields 0 (`lshift(1, 70)` = 0) instead of being masked to 6 bits, as in gawk 5.4.1 (gawk 5.2.1 on x86-64 still shifts by the count modulo 64). The fold of `and`/`or`/`xor` narrows once, after the last argument. awkrs used to work on a sign-wrapped `i64`, so `or(2^54, 1)` printed 18014398509481984, `xor(2^60, 1)` 1152921504606846976, `lshift(1, 70)` 64 and `compl(2^64)` 9223372036854775808. An operand of 2^64 or more is an undefined C cast in gawk; awkrs saturates it as the aarch64 build does (`compl(2^64)` = 0). Under `-M` gawk works on the unbounded integer and so does awkrs: `compl(0)` = -1, `or(2^70, 1)` = 2^70+1, `lshift(1, 70)` = 2^70 (awkrs masked the count and wrapped at 64 bits). |
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
| Unknown conversion letters (`%q`, `%v`, …) | **Match** — the conversion is abandoned: the text from `%` through the letter (`%q`, `%5q`) is emitted literally and no argument is consumed (gawk parity). |
| Spec characters out of the usual order | **Match** — a port of gawk `format_tree`'s `retry` loop: flags, width, precision and the ignored `h`/`l`/`L` modifiers are accepted in any order (`%l5d`, `%5-d`, `%h-5d`), and a character the loop rejects — a repeated modifier (`%lld`), a flag after the precision (`%.3 d`), a second `.` — abandons the spec, which is copied literally while scanning resumes after it (`%ll%d` still converts the `%d`). A format ending inside a spec (`%5`) is literal too. awkrs used to accept modifiers only after the precision, consume an argument for `%lld`, and drop the flags and width from a literal `%5q`. |
| `%a` / `%A` hex float | **Match** (`format_hex_float` in `src/format.rs`; gawk parity confirmed) |
| Non-finite floats (`±inf`, `±nan`) across `%f`/`%e`/`%g`/`%a` | **Match** (gawk-style `+inf` / `-inf` / `+nan` / `-nan`, with `INF` / `NAN` for uppercase variants — `format_non_finite` in `src/format.rs`) |
| `print` of non-finite values | **Match** — `format_number` in `src/runtime.rs` emits the same `+inf` / `+nan` spelling so `print x` and `printf "%s", x` agree |
| `LC_NUMERIC` (`-N`) | **Part** (documented split: format vs parse) |
| `%'` flag thousands grouping | **Match** — consults `localeconv()->thousands_sep` regardless of `-N` (gawk parity). Empty in `LC_ALL=C` → no grouping; `","` in `en_US.UTF-8` → comma grouping. |
| `==` / `<` / `>` of `Num` vs string literal | **Match** — string-compare fallback stringifies the number via `CONVFMT` (not the default `%.6g`). E.g. `BEGIN{CONVFMT="%.2f"; print 3.14159=="3.14"}` prints `1`. |
| `a % 0` / `a %= 0` | **Match** — fatal "division by zero attempted in `%'" (was previously NaN). |
| Numeric coercion of `"inf"` / `"nan"` | **Match** — bare special names coerce to 0; only signed three-letter `inf` / `nan` (case-insensitive) are accepted. `"+infinity"` is rejected like in gawk. |
| Negative bitwise operands | **Match** — fatal (exit 2) with gawk's text: `lshift(-1.000000, 2.000000): negative values are not allowed`, `compl(-3.000000): negative value is not allowed`, and for `and`/`or`/`xor` the right-most negative argument, `and: argument 2 negative value -2 is not allowed` (`argument #2` under `-M`). `and`/`or`/`xor` used to accept a negative operand and wrap it, so `and(-1, 2)` printed 2. |
| `typeof($field)` of noisy numeric text (e.g. `"42abc"`) | **Match** — reports `"string"` (numeric prefix alone is not enough); field comparisons against numbers use string-compare. Pure-numeric text (`"42"`) still reports `"strnum"`. |
| `match(str, re, arr)` start/length subscripts | **Match** — writes `arr[i, "start"]` (1-based char index) and `arr[i, "length"]` for each successful submatch; unmatched optional groups have NO entries. |
| `mktime(spec [, utc])` | **Match** — optional second argument forces UTC interpretation when truthy; one-arg form remains local-time. The fields go to C `mktime`/`timegm` unvalidated, as in gawk, so out-of-range values roll over (`"2024 02 30 0 0 0"` is March 1) and a seventh field is the DST flag. |
| Assignment in ternary else-branch (`1 ? x=1 : x=2`) | **Match** — the else-branch parses as an assignment-expression (gawk grammar). Previously rejected as "invalid assignment target". |
| `asort` / `asorti` on unassigned name | **Match** — treats missing slot as an empty array (returns 0). Scalar values still raise the "first argument is not an array" fatal. Compiler tracks these positions for array-slot promotion. |
| Numeric `==` precision | **Match** — bit-exact (POSIX). Previously used a fuzzy `f64::EPSILON` tolerance, so `0.1 + 0.2 == 0.3` returned true (the difference is ~5.55e-17, below EPSILON). Now matches gawk's 0. |
| Paragraph-mode `RT` (`RS == ""`) | **Match** — captures the FULL run of trailing newlines from the last content line plus the blank lines separating records (`b\n\nc` → RT == "\n\n"). The last record also captures EOF-trailing newlines into RT. |
| `PROCINFO["strftime"]` default | **Match** — `"%a %b %e %H:%M:%S %Z %Y"` (gawk's date(1)-equivalent default), not `"%c"`. |
| `printf("fmt", a, b)` function-call form | **Match** — equivalent to `printf "fmt", a, b`. Previously rejected as "parenthesized comma list is not allowed". Mixed paren-args + bare args (`printf(a,b), c`) still rejected. |
| Builtin called with wrong arity | **Match (no panic)** — uniform `"N is invalid as number of arguments for X"` error across `tolower`, `toupper`, `index`, `substr`, `length`, `system`, `close`, `rand`, `srand`, `asort`, `asorti`, `split`, `match`, `sub`, `gsub`, `exp`, `log`, `sin`, `cos`, `sqrt`, `atan2`, `int`. Earlier awkrs panicked on some, silently ignored extras on others, and used a non-gawk wording on the math functions. A fixed-arity builtin is now rejected **while parsing**, as gawk does, so a `BEGIN` that printed before the bad call produces no output at all; `builtin_arity` in `src/compiler.rs` holds the ranges, each one measured against gawk. The status matches too — every `validate_program` rejection is now `Error::Validate`, which exits **1** like gawk's parse diagnostics instead of 2 (this covers `break`/`continue` outside a loop and a parenthesized comma list as well, all of which gawk also rejects with 1). An *indirect* call carries no name at parse time, so `f = "sin"; @f()` still runs, prints, and fatals at the call with status 2 — which is also what gawk does. |
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
| `printf` `%o` `%u` `%x` `%X` of a value out of the unsigned range | **Match** — gawk's `format_integer_digits` rule for all four: the truncated value goes through `uintmax_t` (`intmax_t` when negative) and is printed in the base only if it survives the round trip, so `%x` of `2^63` is `8000000000000000` and of `2^63 + 2^62` `c000000000000000`; otherwise it falls back to `%g` with the conversion's own flags, width and precision (`%x` of `1e30` is `1e+30`, `%+x` `+1e+30`, `%#x` `1.00000e+30`, `%u` of `2^65` `3.68935e+19`). NaN and infinity print as `+nan`/`-inf` (`+INF` for `%X`), space-padded even under `0`. awkrs went through `i64` for `%o`/`%x`/`%X`, so `%x` of `2^63` and of `1e30` were both `7fffffffffffffff`, and NaN printed `0`. Under `-M` `%o`/`%x`/`%X` print the whole integer (`%x` of `2^64` is `10000000000000000`) instead of its low 64 bits. An operand of 2^64 or more reaches an undefined C cast in gawk: awkrs follows the aarch64 build, which saturates (`%x` of `2^64` is `ffffffffffffffff`); an x86-64 gawk prints `%g` there. |
| MPFR (`-M`) | **Part** (precision / rounding via `PROCINFO`) |
| `printf` precision on `d` `i` `o` `u` `x` `X` | **Match** — the precision is a minimum digit count reached by zero-padding the magnitude, and while it is present the `0` flag is ignored, so `%08.2d` of `42` is `      42`. The `#` prefix goes outside that padding (`%#.5x` of `255` is `0x000ff`), and on `%o` it raises the precision far enough to force a leading zero, so `%#.0o` of `0` is `0` where plain `%.0o` is empty. awkrs previously ignored the precision entirely on `o`/`u`/`x`/`X` and let the `0` flag win on all six. |
| `printf` rounding at an exact half | **Match** — `%e`/`%f`/`%g` round the exact binary value, halves to even, as C does: `%.1g` of `2.5` is `2` and of `4.5` is `4`, while `%.2g` of `1.35` is `1.4` because 1.35 is a shade above the half. `%g` previously rounded in arithmetic (scale, `f64::round`, unscale), which both rounded halves away from zero and moved the value before rounding — `%.1g` of `0.15` came out `0.2` because `0.15 * 10` is exactly `1.5` in `f64` even though `0.15` is not. |
| Byte-exact strings | **Match** — values hold a byte string (`AwkStr`), so a byte that is not part of valid UTF-8 survives `$0`, fields, `substr`, `index`, `length`, `toupper`, concatenation, array subscripts, `split` elements, `~`, `sub`/`gsub`/`gensub`, `printf`/`sprintf` and `print`, and is accepted in the program text itself. See the byte-exact-strings entry in §9 for the verified matrix and what still renders. |
| Regex acceptance set | **Match** — `~` accepts what the references accept rather than what Rust's parser does. `(?:…)` and `(?i)…` are fatal (ERE has no non-capturing group and no inline flags, so the `?` has no preceding expression); `))`, `{`, `a{` and `\Qa\E` are literal text; a reversed range like `[z-a]` is the characters as a set (mawk and one-true-awk, against gawk's fatal). Inside a bracket the character escapes keep their character but the class shorthands do not name a class — `[\t]` is a tab while `[\w]` is the letter `w`, matching neither a backslash nor a digit. POSIX bracket members are members: `[` inside a bracket expression is an ordinary character unless it opens `[:class:]`, `[.c.]` or `[=c=]` (the last two are that character), a `]` first in the list (after an optional `^`) belongs to the set, and `&` / `~` are plain characters — so `[][]`, `[[]` and `[^]]` work; awkrs passed them to Rust's parser, which read the inner `[` as a nested class and died with "unclosed character class". A `/` inside a bracket does not end a regexp literal (`/a[/]b/`), as in gawk and mawk. gawk's regexp operators `\y` (word boundary), `` \` `` and `\'` (start and end of the string) are recognised alongside `\<`, `\>`, `\B`, `\s`, `\S`, `\w`, `\W`; awkrs read the first three as plain characters. |
| `-M` float literals | **Match** — a literal holds the decimal that was written, not the `f64` nearest it, so ten additions of `0.1` come to exactly `1` as in gawk. `%.*f` and `%.*e` emit the number of digits asked for; `rug`'s formatting precision counts significant digits, so `%.4f` of `2.5` used to print `2.500` and `%.0f` the whole binary expansion. |
| `printf "%c"` of a number | **Match** — the low byte in a single-byte locale (`233` → `\351`, `955` → `\273`), the UTF-8 encoding of the code point in a UTF-8 locale. That is gawk in both locales and mawk / one-true-awk in the C locale, which is the only rule no reference contradicts. awkrs previously emitted the encoding regardless of locale. |
| Output already printed when a fatal is raised | **Match** — a fatal does not un-print what the program already wrote, at any phase. Verified on `BEGIN { print "A"; printf "%d\n" }` (a format that outruns its arguments, which gawk, mawk and one-true-awk all treat as fatal): every reference writes `A` and exits 2, and so does awkrs. Output is buffered, and the flush used to happen on a normal exit and on the record loop's error path only, so a fatal raised in `BEGIN`, `BEGINFILE`, `ENDFILE` or `END` dropped the buffer on the way to the process exit and the run appeared to print nothing — silent data loss in a pipeline. Every phase now flushes what it printed before reporting the fatal (`flush_if_err!` in `src/lib.rs`), and the diagnostic still wins over any error the flush itself hits. |

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
- **Exit status**: fatal conditions (runtime faults, an unreadable `-f` file, an input file that cannot be opened, output-redirection I/O errors) exit **2**, matching all three reference awks. Parse diagnostics exit **1**, matching gawk; mawk and one-true-awk exit 2 there. That includes everything `validate_program` rejects before the program runs — wrong builtin arity, `break`/`continue` outside a loop, a parenthesized comma list — which is why those carry `Error::Validate` rather than `Error::Runtime`: they are reported at parse time and never reach the runtime. See `Error::exit_status` in `src/error.rs`.
- **`printf "%c"` with an empty string**: emits nothing, matching POSIX ("the first character of the string value") and one-true-awk. gawk and mawk emit a NUL byte.
- **`"0x10" + 0`**: `0`, matching POSIX, gawk and mawk. one-true-awk's `strtod` accepts the `0x` prefix and yields 16.
- **`printf` unsigned conversions of a negative argument** (`%x` `%o` `%X`): converted as a 64-bit unsigned value (`printf "%x", -3` → `fffffffffffffffd`), matching gawk. one-true-awk and mawk both print `0`.
- **`@namespace` scope**: awkrs applies an `@namespace "ns"` directive to the whole program; gawk applies it from the directive to the end of that source file, so a rule written before the directive (or in an earlier `-f` file) stays in the namespace in effect there. `BEGIN { z = 7 } @namespace "ns" BEGIN { print awk::z }` prints `7` in gawk and an empty line here, because the first rule's `z` became `ns::z`.

- **`OFMT` / `CONVFMT` set to a non-floating-point conversion** (e.g. `"%d"`): undefined by POSIX, and all three references differ — one-true-awk ignores the setting, mawk prints a garbage integer, gawk warns and prints `0`. awkrs produces gawk's value without the warning.
- **Paragraph mode field splitting**: with `RS == ""` a single-character `FS` gains `<newline>` as an additional separator (gawk and one-true-awk both do this; mawk does not). A regex `FS` is left alone in every reference, so an embedded newline stays inside the field.
- **Character semantics are UTF-8, not locale-driven**: `length`/`substr`/`index`/`toupper`/`tolower` — and `match`'s `RSTART`/`RLENGTH`, which report 1-based **character** positions in the same unit — count and fold Unicode scalar values regardless of `LC_ALL`, so `length("é")` is 1 even under `LC_ALL=C` where gawk reports 2. `-b` selects byte semantics explicitly, and it now governs **case folding as well as counting**: under `-b`, `toupper`/`tolower` fold ASCII only, so `toupper("café")` is `CAFé` and `toupper("Straße")` is `STRAßE` — reproducing all three references in the C locale, and keeping the result the same length as its input where Unicode's `ß` → `SS` would grow it. `-b` previously switched the counting builtins but left folding Unicode-aware, so a single `-b` run reported `length("café") == 5` while `toupper("café")` returned `CAFÉ` — the byte world and the character world in one program. Without `-b` the fold is the Unicode **simple** (1:1) mapping, which is the one gawk applies: `toupper("ß ﬁ ŉ")` comes back unchanged and `tolower("İ")` is `i`, so a fold never changes `length()`. awkrs previously used the *full* mapping from `SpecialCasing.txt` and grew the string (`ß` → `SS`, `ﬁ` → `FI`, `ŉ` → `ʼN`, `İ` → `i` plus a combining dot), which no reference awk does in any locale. `printf "%c"` of a numeric argument is the one part of this entry that is **not** locale-independent, because the references do not agree on a single answer: in a single-byte locale gawk, mawk and one-true-awk all emit `N & 0xFF`, and in a UTF-8 locale gawk emits the UTF-8 encoding of the code point while the other two stay on bytes. awkrs follows the locale, which matches gawk in both and the other two in the one they agree with it on. The rule in a single-byte locale is the low byte for **any** `N`, not just below 256: `233` → `e9`, `300` → `2c`, `511` → `ff`, `955` → `bb`, `1000` → `e8`. The only values the references split on there are the ones whose low byte is `00` (`256`, `512`, `1024`, `65536`): gawk and mawk emit the NUL, one-true-awk emits nothing — the same split this section already records for `%c` of an empty string. awkrs used to emit the UTF-8 encoding regardless of locale, which was a genuine C-locale gap rather than a reference disagreement; see the byte-exact-strings entry below.
- **A function name used as a variable** (`function f(){} BEGIN{ f = 1 }`) is rejected before the program runs, matching all three references. The **status** is a three-way split: gawk exits 1 (it diagnoses at parse time), mawk and one-true-awk exit 2. awkrs exits **2** with the other two, because the check runs in `validate_program` after parsing rather than inside the grammar. A function *parameter* that shadows a function name is legal everywhere and stays legal here.
- **`close()` of a pipe** returns the command's exit status, matching gawk and mawk; one-true-awk returns 0. A command killed by a signal reports `256 + signo` in gawk, mawk and one-true-awk alike, and awkrs matches from `close()` and `system()` both.
- **`print > "-"`** creates a file named `-`, matching mawk and one-true-awk; gawk writes to standard output instead. The **input** side has no such split — `getline < "-"` reads standard input in all three references, and in awkrs.
- **`print > "/dev/stdout"`** writes to the program's own standard output, interleaved with plain `print` in program order, which is what all three references do. `close("/dev/stdout")` is where they part: gawk flushes and answers 0 with the stream still usable, while mawk and one-true-awk really close descriptor 1 — one-true-awk silently drops every later line and mawk reports `write failure (Bad file descriptor)`. awkrs follows gawk, the only reading under which a program can keep printing.
- **`print > "/dev/stderr"`** writes through the process's own unbuffered standard error, as gawk does (its `devopen` maps the name to fd 2 and `redirect` shares the `stderr` stream): every line reaches the descriptor at once, in program order with warnings and with flushed standard output. Outside `--traditional`, `/dev/fd/1` and `/dev/fd/2` are the same two streams. `close()` of any of these names answers 0 once a `print` opened it and -1 otherwise, like any redirection; `fflush("/dev/stdout")` and `fflush("/dev/stderr")` answer 0 regardless. awkrs used to open `/dev/stderr` as a file behind its own buffer, so `printf "x" > "/dev/stderr"; print "o"; fflush()` wrote `o` first and a warning overtook every buffered line.
- **`strtonum` of a number or a numeric string** returns that number unchanged, gawk's `do_strtonum`: the input field `017` (a strnum) is 17, while the string constant `"017"` or `$1 ""` takes the leading-`0` octal reading and is 15. awkrs used to send every operand through the string reading. The string reading itself still differs from gawk's `get_numbase` / `nondec2awknum` in three places: leading whitespace before `0x` is skipped (gawk: decimal, so `"  0x10"` is 0), a hex value above `2^64` is 0 (gawk keeps accumulating the double), and the octal and hex scans do not stop at the first invalid character (gawk: `"0x1g"` is 1, `"017x"` is 15, and `"0x1.8p1"` is the hex float 3).
- **`typeof(a[k])`** looks the element up where the array lives — a function's array parameter or local array in its frame — and makes an untyped `a` an array, as gawk's `Op_subscript` does. awkrs used to consult only the globals, so `typeof` of every element of a parameter or local array was `untyped`. gawk also creates the element itself, in a state `typeof` keeps reporting as `untyped` and `length` counts; awkrs has no value for that state and leaves the element absent, so after `typeof(a[1])` `length(a)` is 0 where gawk says 1. `isarray(x)` leaves an untyped `x` untyped (gawk passes it with `Op_push_arg_untyped`); it used to make a later `typeof(x)` say `unassigned`. Function locals and parameters do not yet track the untyped → unassigned transition: `function f(  l){ y = l; print typeof(l) }` prints `untyped` where gawk says `unassigned`, and passing an untyped global to a function marks it `unassigned` even when the function never reads it.
- **`sub` / `gsub` replacement backslashes** follow gawk's `do_sub` scan, left to right: `\\\&` is a literal `\&`, `\\\\` a literal `\\` (four in, two out), `\\&` a `\` followed by the match, `\&` a literal `&`, and any other backslash is copied; under `--posix`, `\&` is `&` and `\\` is `\`. awkrs used to collapse backslash pairs only before an `&`, so a run of four with no `&` after it came out as four. mawk and one-true-awk keep four as well — this case is gawk-only.
- **When an output pipe flushes pending standard output**: opening `print … | "cmd"` flushes whatever awk has buffered, so the child's output cannot overtake lines the program printed first. `close()` does **not** flush again — that is mawk and one-true-awk's timing; gawk flushes at both points, so `print "1"; print "P1" | "cat"; print "2"; close("cat")` orders `2` before the child's output in gawk and after it here. At exit the order is gawk's and mawk's: output pipes and coprocesses are closed (their children waited for) before standard output's last flush, so `{ print | "sort" } END { print "done" }` writes the sorted lines and then `done` (one-true-awk writes `done` first); this holds for `exit` too, where awkrs used to leave a pipe's output to arrive after it had exited. Like gawk, awkrs flushes an output pipe after every `print` unless `PROCINFO["BUFFERPIPE"]` or `PROCINFO[cmd, "BUFFERPIPE"]` exists, and flushes a coprocess after every `print`, so `print "x" |& "cat"; "cat" |& getline` no longer deadlocks.
- **A regex `RS` that can match the empty string**: zero-length matches are not separators. `RS="z?"` reads `abc` as a single record in gawk, mawk, one-true-awk and awkrs. Where a pattern mixes empty and real matches the references split: `RS="x*"` over `aXbxc` gives mawk and one-true-awk two records (`aXb`, `c`) by ignoring the empty matches, while gawk emits an empty record per position; awkrs follows mawk and one-true-awk, which is the same rule that makes the `RS="z?"` case unanimous.
- **`getline < <directory>`** reads the directory's entries, one file name per record, sorted. This is an awkrs extension (the same data `readdir()` returns); gawk and mawk return −1 for a directory and one-true-awk reports an I/O error. A script that means to read a file and is handed a directory therefore sees records here where the references see a failure.
- **Record splitting reads a numeric `FS` / `RS` without `CONVFMT`** — the one part of the `CONVFMT`-coercion rule above that is still open, and it is deliberately partial. `BEGIN { CONVFMT="%.2f"; FS=1.23456 }` splits *records* on the full-precision `1.23456` where all three references split on `1.23`; `RS` behaves the same way, and `OFS` / `ORS` match mawk rather than gawk and one-true-awk. The **explicit** separator forms are all correct — `split(s, a, fs)`, and also the two-argument `split(s, a)` that falls back to `FS` — so within awkrs the same `FS` value can separate a `split()` call and a record differently. That inconsistency is the smaller of the two available ones: reading `FS` is not a single site (the record splitter, the `$0`-assignment path, the field-rebuild path and the JIT host each read it independently, and the splitter caches the value per record), so converting at only some of them would put the interpreter and the JIT tier into disagreement, which §9 treats as a bug in its own right. Converting at *assignment* is not the answer either: `print FS` uses `OFMT` in every reference (`CONVFMT="%.2f"; OFMT="%.3f"; FS=1.23456; print FS` prints `1.235`), so the variable has to keep its numeric identity. `length(FS)` and the other string builtins are already correct, because those go through the coercion above. Repro: `printf 'a1.23b\n' | awk 'BEGIN{CONVFMT="%.2f"; FS=1.23456}{print NF}'` — `2` in gawk/mawk/one-true-awk, `1` here.
- **`typeof` of a never-assigned function parameter**: gawk turns a parameter from `"untyped"` into `"unassigned"` the first time it is read; awkrs reports `"untyped"` throughout. Global scalars and array elements do make that transition (a per-slot "touched" bit in `Runtime::slot_touched`); function locals live in a per-call frame map that has nowhere to record it, and adding a parallel per-frame structure would cost work on every user-function call for one value of one gawk-only introspection builtin.
- **`typeof($0)` before any record is read**: gawk reports `"unassigned"`, awkrs reports `"string"` — `$0` starts as an empty record rather than a distinct never-assigned state.
- **Byte-exact strings.** awkrs values hold an `AwkStr` (`src/awkstr.rs`), a byte string, so a byte that is not part of valid UTF-8 travels through the program the way gawk, mawk and one-true-awk pass it. It used to be a Rust `String`, which by construction cannot hold one, so every such byte became `U+FFFD` on the way in — three bytes out where all three references emit the one they were given, and a value that could never match the byte it came from. `Vec<u8>` was chosen over `bstr::BString` (a dependency for an API written here in a few hundred lines, and it derefs to `[u8]` too, so it breaks the same call sites) and over an enum of `Utf8(String) | Bytes(Vec<u8>)` (one logical string with two representations: equality, hashing and array-subscript identity would all have to normalise across the variants, and a site that forgot would answer wrong silently).

  Verified against all three references under `LC_ALL=C`, on the input `a\351b c\377d` — every line below is unanimous, and `tests/posix_parity_regressions.rs` pins them over **bytes**, because a lossy comparison would turn the byte under test into `U+FFFD` and hold against exactly the bug it exists to catch:

  | what | result |
  |---|---|
  | `{ print }`, `{ print $1 }`, `{ print $2 }` | the bytes, unchanged |
  | `{ print length($0), NF, length($1), index($0,"b") }` | `7 2 3 3` |
  | `{ print substr($0,2,1) }` | the single `\351` |
  | `{ print toupper($0) }` | `A\351B C\377D` — there is no case mapping for an unpaired byte, and `U+FFFD` is not one |
  | `{ printf "%s\n", $0 }` and `{ printf "%c\n", substr($0,2,1) }` | the bytes |
  | `{ x = $1 "-" $2; print x }` and `{ x = sprintf("%s",$0); print x }` | the bytes |
  | `{ a[$1]=1; for (k in a) print k }` | the subscript that was stored |
  | `{ split($0,p," "); print p[2] }` | `c\377d` |
  | `{ print ($0 ~ /^a.b$/) }` on `a\351b` | `1` |
  | `{ print ($1 ~ $2) }` where `$2` is `\351` | `1` |
  | `length("\351")` and `printf "%s", "\351"` | `1`, and the single byte — `"\xNN"` / `"\NNN"` in a literal name a byte, not a character |
  | `{ print ($0 ~ "\xe9") }` on `a\351b` | `1` |
  | `{ gsub(/b/,"Z"); print }` and the same for `sub`, `$1`, a variable and an array element | `a\351Z c\377d` — the bytes it did not replace |
  | `{ gsub(/[ab]/,"[&]"); print $1 }` | `[a]\351[b]` — `&` stands for a matched byte |
  | a program holding the byte in a string literal, a regex literal or a comment | runs, from `argv` and from `-f` alike |
  | `-v x=a\351b` | the three bytes |
  | the byte in **code** position | syntax error, as in all three |
  | a binary line `\377\376\0A` | intact (one-true-awk truncates at the NUL; gawk and mawk do not, so the majority rules) |

  `printf "%c"` of a number follows the locale, which is the only behaviour no reference contradicts: in a single-byte locale all three emit `N & 0xFF` (`233` → `\351`, and it stays the low byte above 255 — `300` → `\054`, `955` → `\273`), while in a UTF-8 locale gawk emits the encoding of the code point and the other two stay on bytes. The regex engine takes the same switch — with Unicode mode off, `.` and the character classes work in single bytes, which is what all three do in a single-byte locale. `locale_numeric::ctype_is_utf8` resolves `LC_ALL`, then `LC_CTYPE`, then `LANG`, the precedence gawk and one-true-awk were both observed to use.

  **What is still rendered rather than carried**, each for a named reason:

  - **The fusevm backend's `sprintf` and `awk_keys`.** `fusevm::Value` carries a `String`, so a value crossing into that backend is rendered. `printf` on that backend is unaffected — it writes into `print_buf` directly. Closing this needs a byte string in fusevm itself, which is upstream of this crate, and it is a shared-VM limit rather than an awkrs one: the same ceiling shows up as lone surrogates in node-js's `JSON.stringify` and in tclrs's `%c`.
  - **`rust { }` blocks and `@include` / `@namespace` / `@load`.** Those four rewrite the program *text* before lexing, so they cannot run over bytes they cannot name. A program that needs both a raw byte and one of them is refused by name rather than silently losing one; a program with neither — which is every ordinary one — is unaffected. The introspection flags (`--dump-tokens`, `--dump-ast`, `--dump-bytecode`, `--disasm`, `--tiers`) and the AOT builder take source as `&str` and so see a rendering; none is on the execution path.
