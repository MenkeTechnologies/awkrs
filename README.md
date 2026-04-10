```
  █████╗ ██╗    ██╗██╗  ██╗██████╗ ███████╗
 ██╔══██╗██║    ██║██║ ██╔╝██╔══██╗██╔════╝
 ███████║██║ █╗ ██║█████╔╝ ██████╔╝███████╗
 ██╔══██║██║███╗██║██╔═██╗ ██╔══██╗╚════██║
 ██║  ██║╚███╔███╔╝██║  ██╗██║  ██║███████║
 ╚═╝  ╚═╝ ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝
```

[![CI](https://github.com/MenkeTechnologies/awkrs/actions/workflows/ci.yml/badge.svg)](https://github.com/MenkeTechnologies/awkrs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/awkrs.svg)](https://crates.io/crates/awkrs)
[![Downloads](https://img.shields.io/crates/d/awkrs.svg)](https://crates.io/crates/awkrs)
[![Docs.rs](https://docs.rs/awkrs/badge.svg)](https://docs.rs/awkrs)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

### `[WORLDS FASTEST AWK BYTECODE ENGINE // PARALLEL RECORD PROCESSOR // RUST CORE]`

 ┌──────────────────────────────────────────────────────────────┐
 │ STATUS: ONLINE &nbsp;&nbsp; THREAT LEVEL: NEON &nbsp;&nbsp; SIGNAL: ████████░░ │
 └──────────────────────────────────────────────────────────────┘

> *"Pattern. Action. Domination."*

`awkrs` runs **pattern → action** programs over input records like POSIX `awk` / GNU `gawk` / `mawk`, with a Cranelift-JIT bytecode VM, parallel record processing, and a CLI that accepts the union of POSIX, gawk, and mawk options.

---

## TABLE OF CONTENTS

- [\[0x00\] SYSTEM SCAN](#0x00-system-scan)
- [\[0x01\] SYSTEM REQUIREMENTS](#0x01-system-requirements)
- [\[0x02\] INSTALLATION](#0x02-installation)
- [\[0x03\] LANGUAGE COVERAGE](#0x03-language-coverage)
- [\[0x04\] MULTITHREADING](#0x04-multithreading--parallel-execution-grid)
- [\[0x05\] BYTECODE VM](#0x05-bytecode-vm--execution-core)
- [\[0x06\] BENCHMARKS](#0x06-benchmarks--combat-metrics-vs-awk--gawk--mawk)
- [\[0x07\] BUILD](#0x07-build--compile-the-payload)
- [\[0x08\] TEST](#0x08-test--integrity-verification)
- [\[0xFF\] LICENSE](#0xff-license)

---

## [0x00] SYSTEM SCAN

**Positioning:** POSIX awk + the gawk extensions that show up in real scripts (`BEGINFILE`/`ENDFILE`, coprocess `|&`, CSV mode, `PROCINFO`/`SYMTAB`/`FUNCTAB`, `@include`/`@load`/`@namespace`, `/inet/tcp|udp`, MPFR via `-M`). Performance goal: beat `awk`/`mawk`/`gawk` on supported workloads — see [§0x06](#0x06-benchmarks--combat-metrics-vs-awk--gawk--mawk).

**Implemented gawk-style CLI flags** (where they differ from gawk, the gap is documented):

| Flag | Behavior |
|---|---|
| `-d`/`--dump-variables` | Dump globals after run (stdout, `-`, or file) |
| `-D`/`--debug` | Static rule/function listing — **not** gawk's interactive debugger |
| `-p`/`--profile` | Wall-clock summary + per-record-rule hit counts (`-j 1` only) — **not** gawk's per-line profiler |
| `-o`/`--pretty-print` | AST pretty-print — **not** gawk's canonical reformatter |
| `-g`/`--gen-pot` | Print and exit before execution |
| `-L`/`-t`/`LINT` | Static lint (extension rules, uninit-var hints, `printf` format checks) |
| `-S`/`--sandbox` | Block `system()`, file redirects, pipes, coprocesses, inet I/O |
| `-l name` | Load `name.awk` from `AWKPATH` (default `.`) |
| `-b` | Byte length for `length`/`substr`/`index` |
| `-n` | `strtonum`-style hex/octal coercion |
| `-s`/`--no-optimize` | Disable Cranelift JIT |
| `-c`/`-P` | Stored on runtime; minimal effect today |
| `-r`/`--re-interval` | Parsed; no runtime effect (regex crate already supports `{m,n}`) |
| `-N`/`--use-lc-numeric` | Locale decimal radix and `%'` grouping in `sprintf`/`printf`/`print`. Does **not** affect string→number parsing |

**Gawk parity gaps to know:**

- **`RS`** — newline by default; one (UTF-8) char = literal delimiter; `RS=""` = paragraph mode; multi-char = gawk regex (`RT` is the matched text). `FIELDWIDTHS` selects fixed-width when non-empty.
- **`PROCINFO`** — refreshed before and after `BEGIN`. Includes gawk-style `platform` (`posix`/`mingw`/`vms`, **not** Rust's `macos`/`linux`), `version`, ids, `errno`, `api_major`/`api_minor`, `argv`, `identifiers`, `FS` (active split mode), `strftime`, `pgrpid`, `groupN`, `mb_cur_max` (Linux `sysconf`), per-input `READ_TIMEOUT`/`RETRY` composite keys with fallback chain → global `PROCINFO["READ_TIMEOUT"]` → `GAWK_READ_TIMEOUT` env. Unix primary record reads `poll` when a timeout applies. With `-M`: `gmp_version`, `mpfr_version`, `prec_min`, `prec_max`. User-set keys persist across the post-`BEGIN` refresh.
- **`PROCINFO["sorted_in"]`** — `@ind_*`/`@val_*` modes, plus user comparator function (2-arg = index sort, 4-arg `(i1, v1, i2, v2)` = value sort). Returns negative/zero/positive like `qsort`.
- **`SYMTAB`** — assignment, `for-in`, `length(SYMTAB)` like gawk's global introspection (not GNU's variable-object references).
- **`@load`** — non-`.awk` paths only accepted for **gawk's bundled extension names** (`filefuncs`, `readdir`, `time`, …) as no-ops; the builtins are native. Arbitrary `.so`/gawkapi modules error at parse time.
- **`-M`/`--bignum`** — MPFR via `rug` (default 256 bits, `PROCINFO["prec"]`/`["roundmode"]` apply). Arithmetic, `sprintf`/`printf` integer formats (no f64/i64 clamp), `int`/`intdiv`/`strtonum`/`++`/`--`, bit ops, transcendentals, `srand` (low 32 bits of previous seed), `CONVFMT`/`OFMT`/`%s`/concat/regex coercion all use MPFR. JIT is disabled in `-M` mode.
- **Unicode vs bytes:** `-b` honored for `length`/`substr`/`index`. Full multibyte field-splitting parity is not audited.

#### HELP // SYSTEM INTERFACE
<img src="assets/awkrs-help.png" alt="awkrs -h cyberpunk help (termshot)" width="100%">

---

## [0x01] SYSTEM REQUIREMENTS

- Rust toolchain (`rustc` + `cargo`)
- A C compiler and `make` for `gmp-mpfr-sys` (pulled in by `rug` for `-M`); typical macOS/Linux setups already satisfy this.

---

## [0x02] INSTALLATION

```sh
cargo install awkrs                                    # from crates.io

git clone https://github.com/MenkeTechnologies/awkrs   # from source
cd awkrs && cargo build --release
```

[awkrs on Crates.io](https://crates.io/crates/awkrs)

**Zsh completion:**

```sh
fpath=(/path/to/awkrs/completions $fpath)
autoload -Uz compinit && compinit
```

---

## [0x03] LANGUAGE COVERAGE

 ┌──────────────────────────────────────────────────────────────┐
 │ SUBSYSTEM: LEXER ████ PARSER ████ COMPILER ████ VM ████     │
 └──────────────────────────────────────────────────────────────┘

- **Rules:** `BEGIN`, `END`, `BEGINFILE`/`ENDFILE`, empty pattern, `/regex/`, expression patterns, range patterns (`/a/,/b/` or `NR==1,NR==5`).
- **Statements:** `if`/`while`/`do…while`/`for` (C-style and `for (i in arr)`), `switch`/`case`/`default` (gawk-style: no fall-through, regex `case /re/`), `print`/`printf` (with `>`, `>>`, `|`, `|&` redirection), `break`, `continue`, `next`, `nextfile`, `exit`, `delete`, `return`, `getline` (primary, `< file`, `<& cmd`, `expr | getline [var]`).
- **`getline` as expression:** value `1` (read), `0` (EOF), `-1` (error), `-2` (gawk retryable I/O when `PROCINFO[input,"RETRY"]` is set). JIT `getline` failures abort the JIT chunk instead of returning `-1`/`-2`.
- **Operators:** arithmetic, comparison, string concat, ternary, `in`, `~`/`!~`, `++`/`--` (prefix/postfix on vars, `$n`, `a[k]`), `^`/`**` (right-associative; unary `+`/`-`/`!` bind looser, so `-2^2` = `-(2^2)`).
- **Primary `/`:** The lexer may emit `/` as division when `regex_mode` is false (e.g. after `=`). At a **primary** position `/` cannot start division (division is a binary operator), so the parser re-reads it as `/regex/` — needed for `gsub(/a/,…)`, `split(…, a, /re/)`, `x = /foo/`, etc.
- **gawk regexp constants:** `@/pattern/` yields a **regexp** value (`typeof` reports `regexp`); `~` uses the pattern as a regex.
- **Data:** fields, scalars, associative arrays (`a[k]`, `a[i,j]` with `SUBSEP`), `ARGC`/`ARGV` (set before `BEGIN`; `ARGV[0]` is the executable, `ARGV[1..]` are file paths). `FS` (regex when multi-char), `FPAT` (gawk-style: non-empty splits by regex match), `split`/`patsplit` (3rd arg accepts regex; `patsplit` 4-arg form populates `seps`). **POSIX record model:** `NF = n` truncates or extends fields and rebuilds `$0` with `OFS`; `$0 = "…"` re-splits and updates `NF`. **`FS`/`FPAT` from literals:** bytecode may store source `"…"` as an internal literal string, but `cached_fs` sync on read still tracks those assignments. **Scalars:** uninitialized variables compare like numeric `0` where POSIX expects dual 0/`""`. **String constants vs input:** program string literals are not *numeric strings* for `<`/`<=`/`>`/`>=` the way `$n` can be; arithmetic still uses longest-prefix string→number (`"3.14abc"+0` → `3.14`). **`split("", arr, fs)`** returns `0` (no empty pseudo-field).
- **Records & env:** `RS`/`RT` as documented above. `ENVIRON`, `CONVFMT`, `OFMT`, `FIELDWIDTHS`, `IGNORECASE` (case-insensitive regex + `==`/`!=`/ordering via `strcoll`), `ARGIND`, `ERRNO`, `LINT`, `TEXTDOMAIN`, `BINMODE`. `PROCINFO`/`FUNCTAB`/`SYMTAB` as in [§0x00](#0x00-system-scan).
- **CLI extensions:** `-k`/`--csv` enables CSV mode (RFC-style quoting, `""` escape) — sets `FS`/`FPAT` and uses a dedicated parser aligned with `gawk --csv`.
- **Builtins:** `length`, `index` (empty needle → `1`, matching gawk), `substr` (gawk/POSIX rules for `start < 1` and length adjustment), `intdiv`, `mkbool`, `split`, `sprintf`/`printf` (flags, `*` and `%n$` positional, gawk `%'`, conversions `%s %d %i %u %o %x %X %f %e %E %g %G %c %%` — `%e`/`%E` use signed two-digit exponents; `%c` uses a string’s first character), `gsub`/`sub`/`match`, `gensub`, `isarray`, `tolower`/`toupper`, `int`, math (`sin` `cos` `atan2` `exp` `log` `sqrt`), `rand`/`srand`, `systime`, `strftime` (0–3 args), `mktime`, `system`, `close`, `fflush`, bit ops (`and` `or` `xor` `lshift` `rshift` `compl`), `strtonum`, `asort`/`asorti`. User-defined `function` with parameter locals.
- **Expressions:** integer literals use gawk rules in source — `0x`/`0X` hex; leading `0` octal when all digits are `0`–`7` (otherwise decimal, e.g. `01238` → 1238); floats with a `.` use a decimal integer part (`077.5` → 77.5). Multidimensional membership `(i,j) in arr` uses a parenthesized comma list (gawk); it may appear alone as a `print` argument to emit several fields.
- **I/O model:** main record loop and unredirected `getline` share one `BufReader` so line order matches POSIX. `exit` from `BEGIN` or a pattern action still runs `END` rules, then exits with the requested code.
- **Locale & pipes:** Unix string compare/order uses `strcoll` (`LC_COLLATE`/`LC_ALL`). `|&` and `<&` run under `sh -c` (mixing `|` and `|&` on the same command is an error). With `-N`, `LC_NUMERIC` applies to `sprintf`/`printf` floats and `%'` grouping; without `-N`, `%'` still uses `localeconv()`'s thousands separator (fallback `,`). `-N` does **not** affect parsing of numeric strings from input.
- **Gawk extras:** `@include`, `@load "*.awk"`, `@namespace "…"` (default identifier prefixing; built-ins exempt), indirect calls (`@name(…)` / `@(expr)(…)`), `/inet/tcp/…` and `/inet/udp/…` client sockets, gettext builtins (`bindtextdomain`, `dcgettext`, `dcngettext` with `.mo` catalogs via the `gettext` crate), `-M`/`--bignum` MPFR.

---

## [0x04] MULTITHREADING // PARALLEL EXECUTION GRID

```
 ┌─────────────────────────────────────────────┐
 │  WORKER 0  ▓▓  CHUNK 0   ██ REORDER QUEUE  │
 │  WORKER 1  ▓▓  CHUNK 1   ██ ──────────────>│
 │  WORKER 2  ▓▓  CHUNK 2   ██  DETERMINISTIC │
 │  WORKER N  ▓▓  CHUNK N   ██  OUTPUT STREAM  │
 └─────────────────────────────────────────────┘
```

Default `-j`/`--threads` is **1**. Pass a higher value when the program is **parallel-safe** (static check: no range patterns, no `exit`/`nextfile`/`delete`, no primary `getline`, no pipe/coproc `getline`, no `asort`/`asorti`, no indirect calls, no `print`/`printf` redirection, no cross-record assignments). Records are processed in parallel via **rayon** and output is **reordered to input order** within each batch so pipelines stay deterministic.

**Regular files** are memory-mapped (`memmap2`) and scanned with the same `RS` rules as the sequential path — no `read()` copy of the whole file. **Stdin** parallel chunks up to `--read-ahead` lines (default 1024) per batch, dispatches to workers, emits in order, then refills.

Workers run the same bytecode VM as the sequential path. The compiled program is shared via `Arc<CompiledProgram>` (one compile, cheap refcount per worker) with per-worker runtime state.

**Fallback:** non-parallel-safe programs run sequentially with a warning when `-j > 1`. Programs that use primary `getline` (including in `BEGIN`) also run sequentially for file input. `END` only sees post-`BEGIN` global state — record-rule mutations from parallel workers are not merged.

---

## [0x05] BYTECODE VM // EXECUTION CORE

 ┌──────────────────────────────────────────────────────────────┐
 │ ARCHITECTURE: STACK VM &nbsp;&nbsp; OPTIMIZATION: PEEPHOLE FUSED     │
 └──────────────────────────────────────────────────────────────┘

awkrs compiles AWK programs into a flat bytecode instruction stream and runs them on a stack VM. Short-circuit `&&`/`||`, control flow, and range patterns resolve to jump-patched offsets at compile time. The string pool interns variable names and string constants for cheap `u32` indexing.

**Cranelift JIT (experimental):** the `jit` module compiles eligible bytecode chunks to native code using Cranelift with `opt_level=speed`. The ABI is `(vmctx, slots, field_fn, array_field_add, var_dispatch, field_dispatch, io_dispatch, val_dispatch) -> f64` — opaque `vmctx` plus seven `extern "C"` callbacks (no thread-local lookups). **Tiered compilation:** chunks count entries and only compile after `AWKRS_JIT_MIN_INVOCATIONS` (default 3; set 1 for first-entry compile). Set `AWKRS_JIT=0` to force the bytecode interpreter.

**Eligible JIT ops:** constants, arithmetic/comparisons, jumps and fused loop tests, slot and HashMap-path scalar ops, field reads (including dynamic `$i` writes in mixed mode), `for-in`, `asort`/`asorti`, `match`, `sub`/`gsub`, `split`/`patsplit`, `getline` (primary, file, coproc), `CallUser`, fused print opcodes (including redirects), `typeof`, and a whitelist of builtins (math, `sprintf`/`printf`, `strftime`, `fflush`/`close`/`system`). Mixed-mode chunks NaN-box `Value` in slots; non-mixed chunks keep slots in Cranelift SSA. Unsupported opcodes fall back to the bytecode loop.

**Peephole fusion** combines common sequences into single opcodes:
- `print $N` → `PrintFieldStdout` (zero-alloc field write)
- `s += $N` → `AddFieldToSlot` (in-place numeric parse)
- `i = i + 1` / `i++` / `++i` → `IncrSlot` (one numeric add, no stack traffic)
- `s += i` between slots → `AddSlotToSlot`
- `$1 "," $2` literal concat → `ConcatPoolStr`
- `NR++` HashMap-path → `IncrVar`

**Inline fast paths** bypass `VmCtx` entirely for single-rule programs with one fused opcode (`{ print $1 }`, `{ s += $1 }`). Memory-mapped files also recognize `{ gsub("lit", "repl"); print }` with literal pattern: when the needle is absent, the loop writes each line from the mapped buffer with `ORS` and skips the VM.

**Raw byte field extraction:** `print $N` with default `FS` scans raw bytes in the mapped file buffer to find the Nth whitespace field, writes it to the output buffer, and appends `Runtime::ors_bytes` — no record copy, no UTF-8 validation.

**Other optimizations:**
- **Indexed slots:** scalars get `u16` slot indices; reads/writes are flat-array indexing instead of `HashMap` lookups (specials like `NR`/`FS`/`OFS` and array names stay on the HashMap path).
- **Zero-copy fields:** fields stored as `(u32, u32)` byte ranges into the record string; owned `String`s only on `set_field`.
- **Direct-to-buffer print:** stdout writes go straight into a 64 KB `Vec<u8>` (flushed at file boundaries) — no per-record `String`, `format!()`, or stdout locking.
- **Cached separators:** `OFS`/`ORS` bytes cached on the runtime, updated only on assignment. The direct-to-buffer stdout `print` path uses the full `ofs_bytes`/`ors_bytes` slices (arbitrary length; not capped at 64 bytes).
- **Byte-level input:** `read_until(b'\n')` into a reusable `Vec<u8>` skips per-line UTF-8 validation.
- **Regex cache:** compiled `Regex` objects cached in a `HashMap<String, Regex>`.
- **`sub`/`gsub`:** when target is `$0`, applies the new record in one step. Literal needles reuse a cached `memmem::Finder`. Constant string operands pass via `Cow` (no per-call alloc).
- **`parse_number`:** fast-paths plain decimal integer field text before falling back to `str::parse::<f64>()`.
- **Slurped input:** newline scanning uses `memchr`.
- **Parallel:** compiled program shared via `Arc` across rayon workers (zero-copy).

---

## [0x06] BENCHMARKS // COMBAT METRICS (vs awk / gawk / mawk)

 ┌──────────────────────────────────────────────────────────────┐
 │ HARDWARE: APPLE M5 MAX &nbsp;&nbsp; OS: macOS &nbsp;&nbsp; ARCH: arm64         │
 └──────────────────────────────────────────────────────────────┘

Measured with [hyperfine](https://github.com/sharkdp/hyperfine). BSD awk (`/usr/bin/awk`), GNU gawk 5.4.0, mawk 1.3.4, awkrs **0.1.26**. **Relative** = mean ÷ fastest mean in that table. awkrs has two rows: default (JIT attempted) vs `AWKRS_JIT=0` (bytecode only). Each table is one `hyperfine` invocation across all five commands on the same 1 M-line input, generated 2026-04-10 UTC by `./scripts/benchmark-vs-awk.sh` and copied verbatim from [`benchmarks/benchmark-results.md`](benchmarks/benchmark-results.md). For the awkrs-only JIT-vs-bytecode A/B see [`benchmarks/benchmark-readme-jit.md`](benchmarks/benchmark-readme-jit.md).

### 1. Throughput: `{ print $1 }` over 1 M lines

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 195.0 ms | 179.8 ms | 221.6 ms | 12.43× |
| gawk | 100.8 ms | 92.8 ms | 115.8 ms | 6.42× |
| mawk | 66.2 ms | 61.9 ms | 78.4 ms | 4.22× |
| awkrs (JIT) | 15.7 ms | 13.3 ms | 19.6 ms | **1.00×** |
| awkrs (bytecode) | 16.1 ms | 13.1 ms | 20.2 ms | 1.03× |

### 2. CPU-bound BEGIN (no input)

`BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }`

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 15.8 ms | 14.0 ms | 18.6 ms | 1.71× |
| gawk | 20.7 ms | 18.8 ms | 22.9 ms | 2.24× |
| mawk | 9.7 ms | 8.3 ms | 11.4 ms | 1.06× |
| awkrs (JIT) | 9.2 ms | 8.4 ms | 12.0 ms | **1.00×** |
| awkrs (bytecode) | 9.6 ms | 8.2 ms | 12.0 ms | 1.04× |

### 3. Sum first column (`{ s += $1 } END { print s }`, 1 M lines)

Cross-record state is not parallel-safe, so awkrs stays single-threaded here. On regular-file input, awkrs uses a **raw byte** path: parses the Nth whitespace field directly from the mmap'd buffer.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 158.5 ms | 147.0 ms | 172.7 ms | 12.27× |
| gawk | 62.9 ms | 58.4 ms | 68.9 ms | 4.87× |
| mawk | 37.5 ms | 33.7 ms | 39.9 ms | 2.90× |
| awkrs (JIT) | 13.0 ms | 11.9 ms | 15.4 ms | 1.01× |
| awkrs (bytecode) | 12.9 ms | 11.5 ms | 16.1 ms | **1.00×** |

### 4. Multi-field print (`{ print $1, $3, $5 }`, 1 M lines, 5 fields/line)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 647.6 ms | 623.5 ms | 686.3 ms | 11.60× |
| gawk | 266.1 ms | 257.4 ms | 301.8 ms | 4.77× |
| mawk | 156.6 ms | 149.8 ms | 170.7 ms | 2.81× |
| awkrs (JIT) | 56.4 ms | 53.1 ms | 61.8 ms | 1.01× |
| awkrs (bytecode) | 55.8 ms | 53.4 ms | 61.6 ms | **1.00×** |

### 5. Regex filter (`/alpha/ { c += 1 } END { print c }`, 1 M lines, no matches)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 191.8 ms | 180.1 ms | 208.9 ms | 17.31× |
| gawk | 351.4 ms | 342.7 ms | 363.3 ms | 31.72× |
| mawk | 19.3 ms | 17.5 ms | 21.8 ms | 1.74× |
| awkrs (JIT) | 11.1 ms | 9.5 ms | 13.5 ms | **1.00×** |
| awkrs (bytecode) | 11.1 ms | 9.5 ms | 14.6 ms | 1.00× |

### 6. Associative array (`{ a[$5] += 1 } END { for (k in a) print k, a[k] }`, 1 M lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 826.2 ms | 792.2 ms | 896.0 ms | 2.43× |
| gawk | 342.4 ms | 330.6 ms | 362.5 ms | 1.01× |
| mawk | 610.0 ms | 588.9 ms | 648.7 ms | 1.79× |
| awkrs (JIT) | 340.0 ms | 324.2 ms | 377.7 ms | **1.00×** |
| awkrs (bytecode) | 343.7 ms | 323.5 ms | 356.7 ms | 1.01× |

### 7. Conditional field (`NR % 2 == 0 { print $2 }`, 1 M lines, 2 fields/line)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 289.1 ms | 263.1 ms | 321.1 ms | 9.58× |
| gawk | 116.1 ms | 111.0 ms | 124.4 ms | 3.85× |
| mawk | 71.1 ms | 66.9 ms | 83.6 ms | 2.36× |
| awkrs (JIT) | 30.2 ms | 28.1 ms | 34.0 ms | **1.00×** |
| awkrs (bytecode) | 30.7 ms | 28.0 ms | 35.5 ms | 1.02× |

### 8. Field computation (`{ sum += $1 * $2 } END { print sum }`, 1 M lines, 2 fields/line)

On regular-file input with default FS, awkrs extracts both fields in a single byte scan and parses them as numbers directly from the mmap'd buffer.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 261.8 ms | 251.4 ms | 280.8 ms | 13.96× |
| gawk | 100.5 ms | 95.3 ms | 109.5 ms | 5.36× |
| mawk | 57.7 ms | 54.5 ms | 61.1 ms | 3.08× |
| awkrs (JIT) | 19.0 ms | 17.6 ms | 23.0 ms | 1.01× |
| awkrs (bytecode) | 18.8 ms | 17.5 ms | 22.8 ms | **1.00×** |

### 9. String concat print (`{ print $3 "-" $5 }`, 1 M lines, 5 fields/line)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 640.8 ms | 611.9 ms | 689.3 ms | 12.68× |
| gawk | 182.2 ms | 168.1 ms | 197.2 ms | 3.61× |
| mawk | 121.0 ms | 113.6 ms | 128.1 ms | 2.39× |
| awkrs (JIT) | 51.0 ms | 49.2 ms | 53.8 ms | 1.01× |
| awkrs (bytecode) | 50.5 ms | 48.8 ms | 54.8 ms | **1.00×** |

### 10. gsub (`{ gsub("alpha", "ALPHA"); print }`, 1 M lines, no matches)

Lines do not contain `alpha`, so this measures **no-match** `gsub` plus `print`. On regular-file input, awkrs uses a **slurp inline** path: byte `memmem` scan + `print` with no VM or per-line `set_field_sep_split` when the literal is absent.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 291.5 ms | 282.3 ms | 300.4 ms | 21.15× |
| gawk | 436.3 ms | 425.7 ms | 459.3 ms | 31.66× |
| mawk | 74.3 ms | 68.8 ms | 84.2 ms | 5.39× |
| awkrs (JIT) | 13.8 ms | 12.8 ms | 16.2 ms | **1.00×** |
| awkrs (bytecode) | 13.9 ms | 12.7 ms | 17.6 ms | 1.01× |

```bash
./scripts/benchmark-vs-awk.sh                              # cross-engine §1–§10 (1 M lines)
AWKRS_BENCH_LINES=5000000 ./scripts/benchmark-vs-awk.sh    # 5 M line sweep
./scripts/benchmark-readme-jit-vs-vm.sh                    # awkrs-only JIT vs bytecode A/B
```

---

## [0x07] BUILD // COMPILE THE PAYLOAD

```bash
cargo build --release
```

`awkrs --help` / `-h` prints a cyberpunk HUD (ASCII banner, status box, taglines, footer) in the style of MenkeTechnologies `tp -h`. ANSI colors apply when stdout is a TTY; set `NO_COLOR` to force plain text.

Regenerate the help screenshot after UI changes: `./scripts/gen-help-screenshot.sh` (needs [termshot](https://github.com/homeport/termshot) on `PATH` and a prior `cargo build`). The capture runs on a PTY with `NO_COLOR` unset and renders at 256 columns.

---

## [0x08] TEST // INTEGRITY VERIFICATION

```bash
cargo test
```

CI runs on pushes and pull requests to `main` via [GitHub Actions](.github/workflows/ci.yml): one Ubuntu lint job (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo doc` with `RUSTDOCFLAGS=-D warnings`) plus a build/test matrix on Ubuntu and macOS.

Coverage spans library unit tests for every module (lexer, parser, format, builtins, interp, vm, jit, compiler, runtime, locale, cli, cyber_help) and integration suites under `tests/` that exercise the gawk-style additions, the slurped-input path, parallel record behavior, and the full CLI surface.

---

## [0xFF] LICENSE

 ┌──────────────────────────────────────────────────────────────┐
 │ MIT LICENSE // UNAUTHORIZED REPRODUCTION WILL BE MET         │
 │ WITH FULL ICE                                                │
 └──────────────────────────────────────────────────────────────┘

---

```
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
░░ >>> JACK IN. MATCH THE PATTERN. EXECUTE THE ACTION. <<< ░░
░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
```

##### created by [MenkeTechnologies](https://github.com/MenkeTechnologies)
