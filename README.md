# awkrs

Awk-style record processor in Rust (union CLI, parallel record engine when safe), created by MenkeTechnologies.

## What it does

`awkrs` runs **pattern → action** programs over input records (lines by default), similar to POSIX `awk`, GNU `gawk`, and `mawk`. The CLI accepts a **union** of common options from those implementations so scripts can pass flags through; not every extension flag changes behavior yet—see `--help`.

## Language coverage

Implemented end-to-end:

- **Rules:** `BEGIN`, `END`, **`BEGINFILE`** / **`ENDFILE`** (gawk-style, per input file), empty pattern, `/regex/`, expression patterns, **range patterns** (`/a/,/b/` or `NR==1,NR==5`).
- **Statements:** `if` / `while` / `for` (C-style and `for (i in arr)`), blocks, **`print`** (with no expressions, prints **`$0`**; **`print … >`** / **`>>`** / **`|`** / **`|&`** redirection), **`printf fmt, expr…`** (statement form, same redirections as **`print`**; no automatic newline—add **`\\n`** in the format), `break`, `continue`, **`next`**, **`exit`**, **`delete`**, **`return`** (inside functions), **`getline`** (primary input, **`getline < file`**, and **`getline <& cmd`** for two-way / coprocess reads).
- **Data:** fields (`$n`, `$NF`), scalars, **associative arrays** (`a[k]`, **`a[i,j]`** with **`SUBSEP`**), **`expr in array`** (membership: right-hand side is the array name), `split`, **`patsplit`** (2–4 args; optional fourth array **`seps`** holds text between successive fields), string/number values.
- **Functions:** builtins (`length`, `index`, `substr`, **`split`**, **`sprintf`** / **`printf`** (flags; **`*`** and **`%n$`** for width/precision/value, including forms like **`%*2$d`**; common conversions `%s` `%d` `%i` `%u` `%o` `%x` `%X` `%f` `%e` `%E` `%g` `%G` `%c` `%%`), **`gsub`** / **`sub`** / **`match`**, `tolower` / `toupper`, `int`, `sqrt`, `rand` / `srand`, `system`, `close`, **`fflush`** (stdout, empty string, open **`>`/`>>`** files, open **`|`** pipes, or open **`|&`** coprocesses)), and **user-defined `function`** with parameters and locals (parameters are local; other names assign to globals, matching classic awk).
- **I/O model:** The main record loop and **`getline` with no redirection** share one **`BufReader`** on stdin or the current input file so line order matches POSIX expectations. **`exit`** sets the process status; **`END` rules still run** after `exit` from `BEGIN` or a pattern action (POSIX-style), then the process exits with the requested code.

## Multithreading

By default **`-j`** / **`--threads`** is set to the CPU count (`num_cpus`). When the program is **parallel-safe** (static check: no range patterns, no `exit`, no primary `getline`, no **`getline <&`** coprocess, no `delete`, no **`print`/`printf` redirection** to files, pipes, or coprocesses, no cross-record assignments or other mutating expressions in record rules or user functions) **and** input comes from **files** (not stdin-only), **records are processed in parallel** with **rayon**; `print` / `printf` output is **reordered to input order** so pipelines stay deterministic. **Stdin** is always read **line-by-line** (streaming); parallel record mode does not buffer all of stdin.

If the program is not parallel-safe, the engine **falls back to sequential** processing and prints a **warning** (use **`-j 1`** to force a single thread and silence the warning). **`END`** still sees only **post-`BEGIN`** global state (record-rule mutations from parallel workers are not merged into the main runtime). Flags **`--read-ahead`** are accepted for CLI compatibility; the prefetch reader thread is not used.

**Tradeoff:** Parallel mode loads each **input file** fully into memory before executing rules (not stdin).

## Zsh Completion

Add the completions directory to your `fpath` before `compinit`:

```zsh
fpath=(/path/to/awkrs/completions $fpath)
autoload -Uz compinit && compinit
```

## Build

```bash
cargo build --release
```

`awkrs --help` / `-h` prints a **cyberpunk HUD** (ASCII banner, status box, taglines, footer) in the style of MenkeTechnologies `tp -h`. ANSI colors apply when stdout is a TTY; set `NO_COLOR` to force plain text.

![`awkrs -h` cyberpunk help (termshot)](assets/awkrs-help.png)

Regenerate the screenshot after UI changes: `./scripts/gen-help-screenshot.sh` (needs [termshot](https://github.com/homeport/termshot) on `PATH` and a prior `cargo build`).

## Test

```bash
cargo test
```

On pushes and pull requests to `main`, [GitHub Actions](.github/workflows/ci.yml) runs `cargo fmt --check`, `cargo clippy` (deny warnings), `cargo test` on Ubuntu and macOS, and `cargo doc` with `RUSTDOCFLAGS=-D warnings`.

Library unit tests cover `format` (including locale decimal radix for float conversions), lexer, and parser; integration tests live in `tests/integration.rs` and `tests/more_integration.rs` with shared helpers in `tests/common.rs`. End-to-end coverage includes the **`in`** operator, **`-N` / `--use-lc-numeric`** with `LC_NUMERIC`, and **stdin vs. file** parallel record behavior.

## Benchmarks (vs awk / gawk / mawk)

Measured with [hyperfine](https://github.com/sharkdp/hyperfine) on **Apple M5 Max** (macOS, `arm64`). BSD awk (`/usr/bin/awk`), GNU awk 5.4.0, mawk 1.3.4, awkrs 0.1.0. Full raw output in [`benchmarks/benchmark-results.md`](benchmarks/benchmark-results.md).

### 1. Throughput: `{ print $1 }` over 200 K lines

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 77.9 ms | 70.9 ms | 89.9 ms | 15.07× |
| gawk | 26.6 ms | 24.9 ms | 31.8 ms | 5.14× |
| mawk | 18.7 ms | 17.1 ms | 22.8 ms | 3.62× |
| awkrs `-j1` | 5.2 ms | 4.8 ms | 6.1 ms | **1.00×** |

### 2. CPU-bound BEGIN (no input)

`BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }`

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 15.4 ms | 13.9 ms | 17.1 ms | 2.93× |
| gawk | 20.1 ms | 18.5 ms | 22.0 ms | 3.81× |
| mawk | 9.3 ms | 8.2 ms | 10.4 ms | 1.77× |
| awkrs | 5.3 ms | 4.8 ms | 5.9 ms | **1.00×** |

### 3. Sum first column (`{ s += $1 } END { print s }`, 200 K lines)

Cross-record state is not parallel-safe, so awkrs is `-j1` only.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 68.4 ms | 64.0 ms | 87.4 ms | 6.74× |
| gawk | 18.4 ms | 17.4 ms | 20.4 ms | 1.82× |
| mawk | 12.3 ms | 11.3 ms | 13.6 ms | 1.21× |
| awkrs `-j1` | 10.1 ms | 9.2 ms | 11.0 ms | **1.00×** |

### 4. Multi-field print (`{ print $1, $3, $5 }`, 200 K lines, 5 fields/line)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 116.1 ms | 107.9 ms | 136.4 ms | 3.96× |
| gawk | 59.5 ms | 55.4 ms | 65.9 ms | 2.03× |
| mawk | 35.1 ms | 32.1 ms | 45.6 ms | 1.20× |
| awkrs `-j1` | 29.3 ms | 26.7 ms | 30.4 ms | **1.00×** |

### 5. Regex filter (`/alpha/ { c += 1 } END { print c }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 137.7 ms | 125.2 ms | 183.5 ms | 19.80× |
| gawk | 97.2 ms | 85.0 ms | 113.6 ms | 13.97× |
| awkrs `-j1` | 14.0 ms | 13.2 ms | 15.5 ms | 2.01× |
| mawk | 7.0 ms | 6.8 ms | 7.3 ms | **1.00×** |

### 6. Associative array (`{ a[$5] += 1 } END { for (k in a) print k, a[k] }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 101.8 ms | 89.4 ms | 126.1 ms | 6.46× |
| awkrs `-j1` | 34.8 ms | 34.3 ms | 35.3 ms | 2.21× |
| gawk | 23.1 ms | 21.5 ms | 24.8 ms | 1.46× |
| mawk | 15.8 ms | 15.3 ms | 16.4 ms | **1.00×** |

### 7. Conditional field (`NR % 2 == 0 { print $2 }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 94.9 ms | 88.9 ms | 106.9 ms | 4.87× |
| gawk | 28.6 ms | 27.2 ms | 30.5 ms | 1.47× |
| awkrs `-j1` | 24.5 ms | 23.8 ms | 26.1 ms | 1.26× |
| mawk | 19.5 ms | 18.0 ms | 21.4 ms | **1.00×** |

### 8. Field computation (`{ sum += $1 * $2 } END { print sum }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 93.6 ms | 85.7 ms | 109.3 ms | 4.85× |
| gawk | 26.6 ms | 25.4 ms | 27.7 ms | 1.38× |
| awkrs `-j1` | 24.8 ms | 23.8 ms | 27.3 ms | 1.29× |
| mawk | 19.3 ms | 18.7 ms | 20.1 ms | **1.00×** |

### 9. String concat print (`{ print $3 "-" $5 }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| BSD awk | 115.2 ms | 105.7 ms | 133.9 ms | 4.33× |
| gawk | 40.8 ms | 38.9 ms | 42.9 ms | 1.54× |
| awkrs `-j1` | 33.4 ms | 31.9 ms | 35.0 ms | 1.26× |
| mawk | 26.6 ms | 25.1 ms | 30.7 ms | **1.00×** |

### 10. gsub (`{ gsub("alpha", "ALPHA"); print }`, 200 K lines)

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| mawk | 18.7 ms | 17.5 ms | 23.4 ms | **1.00×** |
| awkrs `-j1` | 42.3 ms | 35.9 ms | 47.5 ms | 2.26× |
| gawk | 149.1 ms | 146.6 ms | 153.0 ms | 7.97× |
| BSD awk | 151.8 ms | 144.3 ms | 167.1 ms | 8.12× |

> Regenerate after `cargo build --release` (requires `hyperfine`; `gawk` optional):
> ```bash
> ./scripts/benchmark-vs-awk.sh
> ```

**Bytecode VM:** the engine compiles AWK programs into a flat bytecode instruction stream, then runs them on a stack-based virtual machine. This eliminates the recursive AST-walking overhead of a tree interpreter — no per-node pattern matching, no heap pointer chasing through `Box<Expr>`, and better CPU cache locality from contiguous instruction arrays. Short-circuit `&&`/`||` and all control flow (loops, break/continue, if/else) are resolved to jump-patched offsets at compile time. The string pool interns all variable names and string constants so the VM refers to them by cheap `u32` index. **Peephole optimizer:** a post-compilation pass fuses common multi-op sequences into single opcodes — `print $N` becomes `PrintFieldStdout` (writes field bytes directly to the output buffer, zero allocations), `s += $N` becomes `AddFieldToSlot` (parses the field as a number in-place without creating an intermediate `String`), `i = i + 1` becomes `IncrSlot` (one f64 add instead of 5 opcodes with multiple `Value::clone()`), and `s += i` between slot variables becomes `AddSlotToSlot` (two f64 reads + one write, no stack traffic). Jump targets are adjusted automatically after fusion. **Inline fast path:** single-rule programs with one fused opcode (e.g. `{ print $1 }`, `{ s += $1 }`) bypass VmCtx creation, pattern dispatch, and the bytecode execute loop entirely — the operation runs as a direct function call in the record loop. **Raw byte field extraction:** for `print $N` with default FS, the throughput path skips record copy, field splitting, and UTF-8 validation entirely — it scans raw bytes in the slurped file buffer to find the Nth whitespace-delimited field and writes it directly to the output buffer. **Indexed variable slots:** scalar variables are assigned `u16` slot indices at compile time and stored in a flat `Vec<Value>` — variable reads and writes are direct array indexing instead of `HashMap` lookups. Special awk variables (`NR`, `FS`, `OFS`, …) and array names remain on the HashMap path. **Zero-copy field splitting:** fields are stored as `(u32, u32)` byte-range pairs into the record string instead of per-field `String` allocations. Owned `String`s are only materialized when a field is modified via `set_field`. **Direct-to-buffer print:** the stdout print path writes `Value::write_to()` directly into a persistent 64 KB `Vec<u8>` buffer (flushed at file boundaries), eliminating per-record `String` allocations, `format!()` calls, and stdout locking. **Cached separators:** OFS/ORS bytes are cached on the runtime and updated only when assigned, eliminating per-`print` HashMap lookups. **Byte-level input:** records are read with `read_until(b'\n')` into a reusable `Vec<u8>` buffer, skipping per-line UTF-8 validation and `String` allocation. **Regex cache:** compiled `Regex` objects are cached in a `HashMap<String, Regex>` so patterns are compiled once, not per-record. **Parallel** mode shares the compiled program via **`Arc`** across rayon workers (zero-copy); each worker gets its own stack, slots, and runtime overlay.

## Still missing or partial

**Two-way pipe** (**`|&`** / **`getline … <&`**): **`sh -c`** with stdin and stdout connected (same command string for both directions). Mixing **`|`** and **`|&`** on the same command string is an error. On **Unix**, string **`==`**, **`!=`**, and relational ordering use **`strcoll`** (honors **`LC_COLLATE`** / **`LC_ALL`** from the environment). With **`-N`** / **`--use-lc-numeric`**, **`LC_NUMERIC`** is applied (`setlocale(LC_NUMERIC, "")`) and **`sprintf`** / **`printf`** (statement and function) use the locale **decimal radix** for **`%f`** / **`%e`** / **`%g`** / **`%E`** / **`%F`** / **`%G`** output; **`print`** still uses the existing numeric-to-string rules (not full POSIX **`OFMT`** on every `print` yet). Without **`-N`**, numeric formatting in **`sprintf`** uses **`.`**. Exotic **`printf`** combinations not covered above may differ from **gawk**. Many gawk-only extensions are absent. `system()` runs commands via `sh -c` (same caveat as other awks). Prefer validating critical scripts against reference `awk`/`gawk`.

## License

MIT — see `Cargo.toml`.
