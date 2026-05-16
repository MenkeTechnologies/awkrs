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
| `-d` dump-variables | — | — | — | Yes | **Part** (dump after run; format awkrs-specific) |
| `-D` debug | — | — | — | Yes | **Part** (listing/dump; not gawk’s debugger) |
| `-g` gen-pot | — | — | — | Yes | **Match** (awkrs POT generator) |
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
| `FS` `RS` `OFS` `ORS` `OFMT` `CONVFMT` | Yes | Yes | Yes | **Match** |
| `FILENAME` `ARGC` `ARGV` `ENVIRON` | Yes | Yes | Yes | **Match** |
| `SUBSEP` | Yes | Yes | Yes | **Match** |
| `RSTART` `RLENGTH` | Yes | Yes | Yes | **Match** |
| `RT` | No | Part | Yes | **Match** |
| `ARGIND` | No | No | Yes | **Match** |
| `ERRNO` | No | No | Yes | **Match** |
| `PROCINFO` | No | No | Yes | **Part** (keys: `sorted_in`, read timeout, errno, FS mode, bignum, identifiers, etc. — not every gawk key) |
| `SYMTAB` `FUNCTAB` | No | No | Yes | **Part** (reflection best-effort) |
| `FIELDWIDTHS` `FPAT` | Part | Part | Yes | **Match** |
| `IGNORECASE` | Part | Part | Yes | **Match** |
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
| `match` `sub` `gsub` `split` | * | * | * | * | **Match** / **Part** (regex engine = Rust `regex`; subtle differences possible) |
| `tolower` `toupper` | * | * | * | * | **Match** |
| `system` `close` | * | * | * | * | **Part** (sandbox; `close` return semantics) |
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

¹ `strtonum` appears in POSIX awk revision used by gawk; older texts omit it.

---

## 7. `printf` / `print` / numeric formatting

| Topic | awkrs |
|-------|--------|
| `%g` / `%G` | **Match** (ISO C significant-digit semantics in fixed style — `src/format.rs`) |
| `%a` / `%A` hex float | **Part** (if present — verify vs gawk) |
| `LC_NUMERIC` (`-N`) | **Part** (documented split: format vs parse) |
| MPFR (`-M`) | **Part** (precision / rounding via `PROCINFO`) |

---

## 8. Regular expressions

| Topic | awkrs |
|-------|--------|
| Engine | Rust `regex` crate (not literal GNU regex copy). |
| Interval quantifiers `{m,n}` | Enabled ( `-r` is no-op). |
| `IGNORECASE` | Supported for split/match contexts that consult runtime. |
| NUL bytes / binary | **Part** (`-b` / `BINMODE` — exercise before relying on). |

---

## 9. Known intentional or unavoidable divergences

- **JIT** (`src/jit.rs`): When enabled, must match interpreter; if a mismatch is found, treat as a bug in JIT, not as “gawk is wrong.”
- **Parallel mode** (`-j`): Record rules may run concurrently; programs with side effects or dependence on global order are unsafe.
- **Dynamic extensions**: gawk `@load "foo.so"` has no equivalent in awkrs.
- **Process / locale / OS**: `PROCINFO["platform"]` mapping uses `posix`/`mingw` style (`procinfo.rs`), not necessarily gawk’s host string for every OS.
- **For-in order**: Without `-P`, gawk-style `sorted_in` and user comparators apply; hash order still differs across engines when sorting is off.

---

## 10. How to extend this matrix

1. Add a row when a user reports a behavioral delta; cite **minimal** repro and which engine defines the expected result.
2. Prefer a **regression test** under `tests/` over a permanent “known bug” row.
3. Update **`BUILTIN_NAMES`** / `exec_builtin_dispatch` in `src/vm.rs` when adding builtins, then mirror here.

---

*Generated from source audit; not a legal conformance statement.*
