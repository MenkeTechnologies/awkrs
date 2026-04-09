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

---

## [0x00] SYSTEM SCAN

`awkrs` runs **pattern → action** programs over input records (lines by default), similar to POSIX `awk`, GNU `gawk`, and `mawk`. The CLI accepts a **union** of common options from those implementations so scripts can pass flags through; see `--help` for which options affect behavior.

#### HELP // SYSTEM INTERFACE
![`awkrs -h` cyberpunk help (termshot)](assets/awkrs-help.png)

---

## [0x01] SYSTEM REQUIREMENTS

- Rust toolchain // `rustc` + `cargo`

## [0x02] INSTALLATION

#### DOWNLOADING PAYLOAD FROM CRATES.IO

```sh
cargo install awkrs
```

#### COMPILING FROM SOURCE

```sh
git clone https://github.com/MenkeTechnologies/awkrs
cd awkrs
cargo build --release
```

[awkrs on Crates.io](https://crates.io/crates/awkrs)

#### ZSH COMPLETION // TAB-COMPLETE ALL THE THINGS

```sh
# add the completions directory to fpath in your .zshrc
fpath=(/path/to/awkrs/completions $fpath)
autoload -Uz compinit && compinit
```

---

## [0x03] LANGUAGE COVERAGE

 ┌──────────────────────────────────────────────────────────────┐
 │ SUBSYSTEM: LEXER ████ PARSER ████ COMPILER ████ VM ████     │
 └──────────────────────────────────────────────────────────────┘

Implemented end-to-end:

- **Rules:** `BEGIN`, `END`, **`BEGINFILE`** / **`ENDFILE`** (gawk-style, per input file), empty pattern, `/regex/`, expression patterns, **range patterns** (`/a/,/b/` or `NR==1,NR==5`).
- **Statements:** `if` / `while` / **`do … while`** / `for` (C-style and `for (i in arr)`), blocks, **`switch`** / **`case`** / **`default`** (gawk-style: no fall-through; **`case /regex/`** regex match; **`break`** exits the **`switch`** only), **`print`** (with no expressions, prints **`$0`**; **`print … >`** / **`>>`** / **`|`** / **`|&`** redirection), **`printf fmt, expr…`** (statement form, same redirections as **`print`**; no automatic newline—add **`\n`** in the format), `break`, `continue`, **`next`**, **`nextfile`** (skip the rest of the current input file, then continue with the next file—like **`next`** but per file; invalid in **`BEGIN`** / **`END`** / **`BEGINFILE`** / **`ENDFILE`**), **`exit`**, **`delete`**, **`return`** (inside functions), **`getline`** (primary input, **`getline < file`**, and **`getline <& cmd`** for two-way / coprocess reads).
- **Data:** fields (`$n`, `$NF`), scalars, **associative arrays** (`a[k]`, **`a[i,j]`** with **`SUBSEP`**), **`ARGC`** / **`ARGV`** (initialized before **`BEGIN`**: **`ARGV[0]`** is the executable name, **`ARGV[1]`** … are input file paths—none when reading stdin only), **`expr in array`** (membership: right-hand side is the array name), **`FS`** (field separator) and **`FPAT`** (gawk-style: non-empty **FPAT** splits `$0` by regex matches—each match is one field; empty **FPAT** uses **FS**), **`split`** (third argument and **`FS`** support **regex** when multi-character, per POSIX), **`patsplit`** (2–4 args; optional fourth array **`seps`** holds text between successive fields), string/number values. **Increment/decrement** (gawk-style): **`++` / `--`** as prefix or postfix on variables, **`$n`**, and **`a[k]`** (numeric coercion per awk rules).
- **CLI (gawk-style):** **`-k`** / **`--csv`** enables **CSV mode** (comma-separated records, double-quoted fields, **`""`** for a literal **`"`** in a field): sets **`FS`** and **`FPAT`** like GNU awk’s introspection, and splits records with a dedicated CSV parser aligned with **`gawk --csv`** (quoted commas do not start a new field). Applied after **`-v`** / **`-F`** so it can override those for CSV workflows.
- **Functions:** builtins (`length`, `index`, `substr`, **`split`**, **`sprintf`** / **`printf`** (flags; **`*`** and **`%n$`** for width/precision/value, including forms like **`%*2$d`**; common conversions `%s` `%d` `%i` `%u` `%o` `%x` `%X` `%f` `%e` `%E` `%g` `%G` `%c` `%%`), **`gsub`** / **`sub`** / **`match`**, `tolower` / `toupper`, `int`, POSIX math (**`sin`**, **`cos`**, **`atan2`**, **`exp`**, **`log`**), `sqrt`, `rand` / `srand`, **`systime()`**, **`strftime`** (0–3 args, gawk-style), **`mktime`** (string datespec), `system`, `close`, **`fflush`** (stdout, empty string, open **`>`/`>>`** files, open **`|`** pipes, or open **`|&`** coprocesses), gawk-style bitwise **`and`** / **`or`** / **`xor`** / **`lshift`** / **`rshift`** / **`compl`**, **`strtonum`** (hex **`0x…`**, leading-zero octal, else decimal), **`asort`** / **`asorti`** (sort an array by value or by key into **`"1"`…`"n"`** indices, optional second destination array)), and **user-defined `function`** with parameters and locals (parameters are local; other names assign to globals, matching classic awk).
- **I/O model:** The main record loop and **`getline` with no redirection** share one **`BufReader`** on stdin or the current input file so line order matches POSIX expectations. **`exit`** sets the process status; **`END` rules still run** after `exit` from `BEGIN` or a pattern action (POSIX-style), then the process exits with the requested code.
- **Locale & pipes:** On Unix, string **`==`**, **`!=`**, and relational ordering use **`strcoll`** (honors **`LC_COLLATE`** / **`LC_ALL`**). **`|&`** / **`getline … <&`** run the command under **`sh -c`** with stdin and stdout connected; mixing **`|`** and **`|&`** on the same command string is an error. **`system(cmd)`** runs **`cmd`** via **`sh -c`**. With **`-N`** / **`--use-lc-numeric`**, **`LC_NUMERIC`** is applied and **`sprintf`** / **`printf`** use the locale decimal radix for float conversions (**`%f`** / **`%e`** / **`%g`** / **`%E`** / **`%F`** / **`%G`**); without **`-N`**, those conversions use **`.`**.

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

By default **`-j`** / **`--threads`** is **1**. Pass a higher value when the program is **parallel-safe** (static check: no range patterns, no `exit`, no **`nextfile`**, no primary `getline`, no **`getline <&`** coprocess, no `delete`, no **`asort`** / **`asorti`**, no **`print`/`printf` redirection** to files, pipes, or coprocesses, no cross-record assignments or other mutating expressions in record rules or user functions); then **records are processed in parallel** with **rayon** and `print` / `printf` output is **reordered to input order** within each batch so pipelines stay deterministic. **Regular files** are **memory-mapped** (`memmap2`) and scanned into per-record `String`s for workers—no extra `read()` copy of the whole file into a heap `Vec<u8>`, with the OS paging large inputs. **Stdin-only** input uses **chunked streaming**: up to **`--read-ahead`** lines (default **1024**) are buffered, that batch is dispatched to workers, output is emitted in order, then the next batch is read—so parallel speedups apply to piped input without loading all of stdin. Parallel workers execute the **same bytecode VM** as the sequential path (`vm_pattern_matches` / `vm_run_rule`); the compiled program is shared via **`Arc<CompiledProgram>`** (one compile, cheap refcount per worker) with **per-worker** runtime state (slots, VM stack, field buffers, captured print lines).

If the program is not parallel-safe, the engine **falls back to sequential** processing and prints a **warning** when **`-j`** is greater than **1** (use a **single thread** to silence the warning). **`END`** still sees only **post-`BEGIN`** global state (record-rule mutations from parallel workers are not merged into the main runtime).

**Tradeoff:** Parallel mode still builds one **`String` per record** for workers; the **file bytes** are mapped read-only, not duplicated in a heap buffer. Stdin parallel mode buffers **`--read-ahead`** lines at a time (not the full stream).

---

## [0x05] BYTECODE VM // EXECUTION CORE

 ┌──────────────────────────────────────────────────────────────┐
 │ ARCHITECTURE: STACK VM &nbsp;&nbsp; OPTIMIZATION: PEEPHOLE FUSED     │
 └──────────────────────────────────────────────────────────────┘

The engine compiles AWK programs into a flat bytecode instruction stream, then runs them on a stack-based virtual machine. This eliminates the recursive AST-walking overhead of a tree interpreter — no per-node pattern matching, no heap pointer chasing through `Box<Expr>`, and better CPU cache locality from contiguous instruction arrays. Short-circuit `&&`/`||` and all control flow (loops, break/continue, if/else) are resolved to jump-patched offsets at compile time. The string pool interns all variable names and string constants so the VM refers to them by cheap `u32` index.

**Cranelift JIT (experimental):** The **`jit`** module uses **Cranelift** + **`cranelift-jit`** to compile eligible bytecode chunks into native code with ABI **`(slots, field_fn, array_field_add, var_dispatch, field_dispatch, io_dispatch, val_dispatch) -> f64`** — seven **`extern "C"`** callback pointers covering field reads, fused array updates, HashMap-path scalar ops, dynamic-field mutations, print side-effects, and multiplexed match/signal/iterator operations. Callers pass a **`JitRuntimeState`** (mutable **`f64`** slot slice + those seven callbacks). Eligible ops include constants, slot and HashMap-path scalar ops (when numeric; **`GetVar`** forces mixed mode for the whole chunk so locals/globals and **`return`** preserve full **`Value`** semantics on the JIT stack), arithmetic and comparisons, jumps and fused loop tests, field access (constant **`PushFieldNum`**, dynamic **`GetField`**, NR/FNR/NF, fused **`AddFieldToSlot`** / **`AddMulFieldsToSlot`**), fused **`ArrayFieldAddConst`** (numeric field index only), general array subscripts and string ops in **mixed mode** (NaN-boxed string handles on the stack; **`val_dispatch`** opcodes ≥ 100 **`MIXED_*`** — including fused **`IncrSlot`** / **`AddFieldToSlot`** / **`JumpIfSlotGeNum`** / related slot peepholes when the chunk is mixed, so numeric-string slots coerce like **`Value::as_number`**; dynamic **`$i = …`** / **`$i += …`** use the same **`MIXED_*`** path when the chunk is mixed; multidimensional **`a[i,j]`** keys use **`JoinArrayKey`** → **`MIXED_JOIN_*`** with **`SUBSEP`**), **`typeof`** on scalars, fields, array elements, and arbitrary expressions (**`MIXED_TYPEOF_*`**), a **whitelist** of builtins via **`Op::CallBuiltin`** (**`MIXED_BUILTIN_ARG`** / **`MIXED_BUILTIN_CALL`** — math/string helpers, **`sprintf`** / **`printf`**, **`strftime`**, **`fflush`** / **`close`** / **`system`**, **`typeof`**, with a capped arg count), **`split(s, arr [, fs])`** (**`Op::Split`** — **`MIXED_SPLIT`** / **`MIXED_SPLIT_WITH_FS`**), **`patsplit`** (**`Op::Patsplit`** — multiple **`MIXED_PATSPLIT_*`**; FPAT + **`seps`** packs two pool indices in **`a1`** when both are &lt; 65536, otherwise stash + **`MIXED_PATSPLIT_FP_SEP_WIDE`**), **`match(s, re [, arr])`** (**`Op::MatchBuiltin`**), **`$n`** compound / **`++$n`** / **`$n++`**, **fused print opcodes** (**`PrintFieldStdout`**, **`PrintFieldSepField`**, **`PrintThreeFieldsStdout`**, bare **`print`**, **`print`** / **`printf`** with arguments on stdout in mixed mode — **`printf`** uses **`MIXED_PRINTF_FLUSH`**; redirects (`>`, `>>`, `|`, `|&`) use **`MIXED_PRINT_FLUSH_REDIR`** / **`MIXED_PRINTF_FLUSH_REDIR`** with **`pack_print_redir`**), **`MatchRegexp`** pattern tests, **flow signals** (**`Next`**, **`NextFile`**, **`ExitDefault`**, **`ExitWithCode`**, **`ReturnVal`**, **`ReturnEmpty`**), **`for`-`in` iteration** (**`ForInStart`** / **`ForInNext`** / **`ForInEnd`** — iterator state in thread-local, key stored in variable via callback), and **`asort`** / **`asorti`** (array sorting via callback, returning count), **`CallUser`** (**`MIXED_CALL_USER_ARG`** / **`MIXED_CALL_USER_CALL`**), and **`sub`**/**`gsub`** (**`MIXED_SUB_*`** / **`MIXED_GSUB_*`** — record, slot/var, field, or array index). The **`io_dispatch`** callback handles fused print opcodes that only touch fields as void side-effects. The **`val_dispatch`** callback multiplexes **`MatchRegexp`** (regex tested against `$0`), mixed-mode string/array/regex/print-arg operations (**`MIXED_*`**), flow signals (set a thread-local flag; the VM translates to **`VmSignal::Next`** / **`ExitPending`** / **`Return`** etc. after JIT returns), **`ForIn`** iteration (collecting array keys, advancing the iterator, storing the current key in the loop variable), and **`asort`** / **`asorti`**. The VM tries the JIT for whole chunks that pass **`is_jit_eligible`**; set **`AWKRS_JIT=0`** to force the bytecode interpreter (e.g. JIT vs VM benchmarks) (mixed chunks mirror **`Value`** in slots via NaN-boxing — **`Value::Uninit`** uses a dedicated quiet-NaN tag, not raw **`0.0`**; flow **`ReturnVal`** decodes the returned **`f64` as a full **`Value`** (including dynamic strings) before clearing thread-local string storage; callbacks use thread-local **`Runtime`** / **`CompiledProgram`** / **`VmCtx`** pointers so semantics match the interpreter). **`CallUser`** and **`sub`**/**`gsub`** compile in mixed mode when **`jit_call_builtins_ok`** and stack rules pass; nested JIT re-entry saves/restores thread-local JIT slot pointers so inner **`execute`** runs do not corrupt the outer chunk. Unsupported opcodes still fall back to the bytecode loop. **`getline`** (primary input, **`getline < file`**, **`getline <&` coproc**) compiles via **`MIXED_GETLINE_*`** when the chunk is otherwise eligible. The library-only legacy helpers **`try_compile_numeric_expr`** / **`is_numeric_stack_eligible`** accept straight-line constant stack math (no jumps — excludes **`Jump`**, **`JumpIf*`**, **`JumpIfSlotGeNum`**, …) including **`%`**, comparisons (**`Cmp*`**), **`Not`** / **`ToBool`**, unary **`+`**, **`Dup`**, **`GetField`**, **`GetSlot`** / **`SetSlot`** / **`CompoundAssignSlot`**, fused **`IncrSlot`** / **`DecrSlot`** / **`AddSlotToSlot`** / **`AddFieldToSlot`** / **`AddMulFieldsToSlot`**, **`PushFieldNum`**, and **`GetNR`** / **`GetFNR`** / **`GetNF`** — same ops the full JIT already compiled; slot storage is sized by **`numeric_stack_slot_words`**; **`call_f64`** still uses stub callbacks (fields/NR return `0.0`; use the VM for real I/O and specials).

**Peephole optimizer:** a post-compilation pass fuses common multi-op sequences into single opcodes — `print $N` becomes `PrintFieldStdout` (writes field bytes directly to the output buffer, zero allocations), `s += $N` becomes `AddFieldToSlot` (parses the field as a number in-place without creating an intermediate `String`), `i = i + 1` / `i++` / `++i` becomes `IncrSlot` and `i--` / `--i` becomes `DecrSlot` (one f64 add instead of push+pop stack traffic), `s += i` between slot variables becomes `AddSlotToSlot` (two f64 reads + one write, no stack traffic), `$1 "," $2` string literal concatenation becomes `ConcatPoolStr` (appends the interned string directly to the TOS buffer — no clone from the string pool), and HashMap-path `NR++` / `NR--` statements fuse to `IncrVar` / `DecrVar` (skip pushing a result that's immediately discarded). Jump targets are adjusted automatically after fusion.

**Inline fast path:** single-rule programs with one fused opcode (e.g. `{ print $1 }`, `{ s += $1 }`) bypass VmCtx creation, pattern dispatch, and the bytecode execute loop entirely — the operation runs as a direct function call in the record loop. Memory-mapped **regular files** also recognize `{ gsub("lit", "repl"); print }` on `$0` with a literal pattern and simple replacement: when the needle is absent, the loop writes each line from the mapped buffer with **ORS** and skips VM + field split.

**Raw byte field extraction:** for `print $N` with default FS, the throughput path skips record copy, field splitting, and UTF-8 validation entirely — it scans raw bytes in the mapped file buffer to find the Nth whitespace-delimited field and writes it directly to the output buffer.

**Indexed variable slots:** scalar variables are assigned `u16` slot indices at compile time and stored in a flat `Vec<Value>` — variable reads and writes are direct array indexing instead of `HashMap` lookups. Special awk variables (`NR`, `FS`, `OFS`, …) and array names remain on the HashMap path.

**Zero-copy field splitting:** fields are stored as `(u32, u32)` byte-range pairs into the record string instead of per-field `String` allocations. Owned `String`s are only materialized when a field is modified via `set_field`.

**Direct-to-buffer print:** the stdout print path writes `Value::write_to()` directly into a persistent 64 KB `Vec<u8>` buffer (flushed at file boundaries), eliminating per-record `String` allocations, `format!()` calls, and stdout locking.

**Cached separators:** OFS/ORS bytes are cached on the runtime and updated only when assigned, eliminating per-`print` HashMap lookups.

**Byte-level input:** records are read with `read_until(b'\n')` into a reusable `Vec<u8>` buffer, skipping per-line UTF-8 validation and `String` allocation.

**Regex cache:** compiled `Regex` objects are cached in a `HashMap<String, Regex>` so patterns are compiled once, not per-record.

**Field split (lazy path):** `ensure_fields_split` reads **`FS`** / **`FPAT`** when splitting (or uses the CSV scanner when **`-k`** / **`--csv`** set `Runtime::csv_mode`); `cached_fs` is still updated when the record is set for bookkeeping.

**`sub` / `gsub`:** when the target is `$0`, the engine applies the new record in one step (no restore-then-overwrite of the old string). Literal patterns with zero matches skip `set_field_sep_split`; literal needles reuse a cached **`memmem::Finder`** for the scan (no `str::contains` per line). `sub`/`gsub` VM opcodes pass pattern/replacement `&str` via `Cow` so constant string operands do not allocate per call.

**Numeric fields:** `parse_number` fast-paths plain decimal integer field text (common for `seq`-style data) before falling back to `str::parse::<f64>()`.

**Slurped input:** newline scanning in the file fast paths uses the `memchr` crate for byte search.

**Parallel** mode shares the compiled program via **`Arc`** across rayon workers (zero-copy); each worker gets its own stack, slots, and runtime overlay.

---

## [0x06] BENCHMARKS // COMBAT METRICS (vs awk / gawk / mawk)

 ┌──────────────────────────────────────────────────────────────┐
 │ HARDWARE: APPLE M5 MAX &nbsp;&nbsp; OS: macOS &nbsp;&nbsp; ARCH: arm64         │
 └──────────────────────────────────────────────────────────────┘

Measured with [hyperfine](https://github.com/sharkdp/hyperfine) (`--shell none` for spot-checks below). BSD awk (`/usr/bin/awk`), GNU gawk 5.4.0, mawk 1.3.4, awkrs **0.1.13**. **Relative** = mean time ÷ **fastest** mean in that table. **awkrs** has two rows: default (**JIT** attempted) vs **`AWKRS_JIT=0`** (**bytecode** only) — same means as [`benchmarks/benchmark-readme-jit.md`](benchmarks/benchmark-readme-jit.md) from `./scripts/benchmark-readme-jit-vs-vm.sh` (regenerate to refresh awkrs columns). **§6** is a single hyperfine run with all five engines on the same 200 K-line input; **§1–§5** and **§7–§10** still mix an older BSD/gawk/mawk sheet with refreshed awkrs rows unless you re-measure everything together. [`benchmarks/benchmark-results.md`](benchmarks/benchmark-results.md) is from `./scripts/benchmark-vs-awk.sh` (cross-engine §1–§4 plus JIT vs bytecode §4a–§4d).

### 1. Throughput: `{ print $1 }` over 200 K lines

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 39.8 ms | 35.3 ms | 55.5 ms | 6.03× |
| gawk | 23.9 ms | 21.7 ms | 29.1 ms | 3.62× |
| mawk | 14.7 ms | 13.2 ms | 18.9 ms | 2.23× |
| awkrs (JIT) | 7.4 ms | 5.9 ms | 9.8 ms | 1.12× |
| awkrs (bytecode) | 6.6 ms | 4.9 ms | 9.0 ms | **1.00×** |

### 2. CPU-bound BEGIN (no input)

`BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }`

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| gawk | 19.6 ms | 17.4 ms | 22.2 ms | 2.61× |
| BSD awk | 14.9 ms | 13.0 ms | 17.8 ms | 1.99× |
| mawk | 9.0 ms | 7.5 ms | 10.4 ms | 1.20× |
| awkrs (JIT) | 12.9 ms | 11.0 ms | 18.7 ms | 1.72× |
| awkrs (bytecode) | 7.5 ms | 6.4 ms | 9.3 ms | **1.00×** |

### 3. Sum first column (`{ s += $1 } END { print s }`, 200 K lines)

Cross-record state is not parallel-safe, so awkrs stays **single-threaded** (default) here.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 32.0 ms | 28.8 ms | 46.5 ms | 3.56× |
| gawk | 16.5 ms | 14.8 ms | 22.4 ms | 1.83× |
| mawk | 9.0 ms | 8.2 ms | 10.6 ms | **1.00×** |
| awkrs (JIT) | 13.5 ms | 12.1 ms | 16.2 ms | 1.50× |
| awkrs (bytecode) | 13.4 ms | 11.9 ms | 16.1 ms | 1.49× |

### 4. Multi-field print (`{ print $1, $3, $5 }`, 200 K lines, 5 fields/line)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 120.8 ms | 113.8 ms | 132.1 ms | 6.75× |
| gawk | 58.4 ms | 53.2 ms | 66.8 ms | 3.26× |
| mawk | 33.2 ms | 30.5 ms | 38.5 ms | 1.85× |
| awkrs (JIT) | 18.1 ms | 16.6 ms | 20.2 ms | 1.01× |
| awkrs (bytecode) | 17.9 ms | 16.8 ms | 20.3 ms | **1.00×** |

### 5. Regex filter (`/alpha/ { c += 1 } END { print c }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 127.2 ms | 115.7 ms | 150.8 ms | 27.06× |
| gawk | 80.9 ms | 79.2 ms | 90.1 ms | 17.21× |
| mawk | 5.6 ms | 4.8 ms | 7.6 ms | 1.19× |
| awkrs (JIT) | 5.2 ms | 4.0 ms | 10.7 ms | 1.11× |
| awkrs (bytecode) | 4.7 ms | 3.5 ms | 6.9 ms | **1.00×** |

### 6. Associative array (`{ a[$5] += 1 } END { for (k in a) print k, a[k] }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 164.4 ms | 155.8 ms | 177.0 ms | 3.03× |
| gawk | 75.3 ms | 71.4 ms | 85.4 ms | 1.39× |
| mawk | 81.6 ms | 69.4 ms | 111.0 ms | 1.51× |
| awkrs (JIT) | 72.8 ms | 64.4 ms | 88.6 ms | 1.34× |
| awkrs (bytecode) | 54.2 ms | 44.2 ms | 74.0 ms | **1.00×** |

### 7. Conditional field (`NR % 2 == 0 { print $2 }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 105.4 ms | 97.8 ms | 136.5 ms | 7.87× |
| gawk | 28.7 ms | 26.7 ms | 32.3 ms | 2.14× |
| mawk | 17.8 ms | 16.2 ms | 21.0 ms | 1.33× |
| awkrs (JIT) | 13.4 ms | 11.6 ms | 16.8 ms | **1.00×** |
| awkrs (bytecode) | 13.5 ms | 11.5 ms | 15.9 ms | 1.01× |

### 8. Field computation (`{ sum += $1 * $2 } END { print sum }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 104.2 ms | 94.9 ms | 133.9 ms | 6.81× |
| gawk | 25.6 ms | 24.0 ms | 27.6 ms | 1.67× |
| mawk | 17.4 ms | 16.1 ms | 22.7 ms | 1.14× |
| awkrs (JIT) | 15.4 ms | 13.4 ms | 20.5 ms | 1.01× |
| awkrs (bytecode) | 15.3 ms | 13.5 ms | 17.7 ms | **1.00×** |

### 9. String concat print (`{ print $3 "-" $5 }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 120.0 ms | 114.7 ms | 139.5 ms | 6.74× |
| gawk | 42.3 ms | 38.3 ms | 90.0 ms | 2.38× |
| mawk | 25.3 ms | 23.0 ms | 30.3 ms | 1.42× |
| awkrs (JIT) | 17.8 ms | 15.9 ms | 22.0 ms | **1.00×** |
| awkrs (bytecode) | 17.9 ms | 15.8 ms | 21.5 ms | 1.01× |

### 10. gsub (`{ gsub("alpha", "ALPHA"); print }`, 200 K lines)

Input lines do not contain `alpha`, so this measures **no-match** `gsub` plus `print` (still scans each line for the literal). On **regular file** input, awkrs uses a **slurp inline** path: byte `memmem` scan + `print` without VM or per-line `set_field_sep_split` when the literal is absent.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 162.2 ms | 154.7 ms | 187.6 ms | 28.46× |
| gawk | 106.8 ms | 105.0 ms | 110.8 ms | 18.74× |
| mawk | 17.3 ms | 15.6 ms | 22.1 ms | 3.04× |
| awkrs (JIT) | 5.9 ms | 4.7 ms | 8.2 ms | 1.04× |
| awkrs (bytecode) | 5.7 ms | 4.8 ms | 7.5 ms | **1.00×** |

> Regenerate after `cargo build --release` (requires `hyperfine`; `gawk` optional). Includes **§4** JIT vs bytecode rows (`AWKRS_JIT=0` disables JIT for A/B).
> ```bash
> ./scripts/benchmark-vs-awk.sh
> ./scripts/benchmark-readme-jit-vs-vm.sh
> ```

---

## [0x07] BUILD // COMPILE THE PAYLOAD

```bash
cargo build --release
```

`awkrs --help` / `-h` prints a **cyberpunk HUD** (ASCII banner, status box, taglines, footer) in the style of MenkeTechnologies `tp -h`. ANSI colors apply when stdout is a TTY; set `NO_COLOR` to force plain text.

Regenerate the screenshot after UI changes: `./scripts/gen-help-screenshot.sh` (needs [termshot](https://github.com/homeport/termshot) on `PATH` and a prior `cargo build`). The capture runs `awkrs -h` on a PTY with `NO_COLOR` unset so ANSI matches a normal TTY (many shells export `NO_COLOR=1`, which would otherwise strip color). [termshot](https://github.com/homeport/termshot) renders the raw capture at 256 columns so long lines are not wrapped.

---

## [0x08] TEST // INTEGRITY VERIFICATION

```bash
cargo test
```

On pushes and pull requests to `main`, [GitHub Actions](.github/workflows/ci.yml) runs `cargo fmt --check`, `cargo clippy` (deny warnings), `cargo test` on Ubuntu and macOS, and `cargo doc` with `RUSTDOCFLAGS=-D warnings`.

Library unit tests cover `format` (including locale decimal radix for float conversions), the lexer, the parser (including error paths), **`Error` diagnostics**, **`cli::Args`** (including **`-W`** / **`mawk` compatibility**), **`builtins`** (`gsub`, `sub`, `match`, `patsplit`, literal-pattern helpers), **`interp`** (pattern matching, range steps, `BEGIN` execution), **`vm`** (BEGIN/END, pattern evaluation, rule actions with print capture, user calls), **`jit`** (Cranelift codegen: numeric, print, array, match, signals), **`lib`** helpers used by the file reader and fast paths (`read_all_lines`, `uses_primary_getline`, NR-mod pattern detection, float compare), **`cyber_help`** layout strings, `locale_numeric` on non-Unix targets, parallel-record static safety in `ast::parallel`, bytecode (`StringPool`, slot init), compiler smoke checks (including `BEGINFILE`/`ENDFILE`, `while`/`if`, deletes, multiple functions), and `runtime::Value` helpers. Integration tests live in `tests/integration.rs`, `tests/more_integration.rs`, `tests/extra_integration.rs`, and `tests/batch_integration.rs`, with shared helpers in `tests/common.rs` (including **file-argument** runs that exercise the slurped-input path). End-to-end coverage includes the **`in`** operator, **`-F` / `--field-separator`** (including **regex FS** like `[,:]`), **`split()` with regex third argument**, **regex literal escaped-backslash** edge cases, **`getline var` NF preservation**, **`-f` / `-i` program sources**, **`-N` / `--use-lc-numeric`** with `LC_NUMERIC`, **`-v` / `--assign`**, **`--version`** / **`-V`**, **`-C`**, coprocess and pipe I/O, and **stdin vs. file** parallel record behavior.

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
