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

## Benchmarks (vs awk / gawk)

Results are **not checked into the README as numbers** (they go stale and vary by machine). The latest run from the maintainer’s environment is in [`benchmarks/benchmark-results.md`](benchmarks/benchmark-results.md) (tables produced by [hyperfine](https://github.com/sharkdp/hyperfine)).

Regenerate after `cargo build --release` (requires `hyperfine` on `PATH`; `gawk` is optional but included when found):

```bash
./scripts/benchmark-vs-awk.sh
```

This compares **BSD awk**, **gawk** (if present), and **awkrs** (`-j1` and parallel where applicable) on three workloads: line throughput, a CPU-heavy `BEGIN`, and a summing pass with `END`.

**Bytecode VM:** the engine compiles AWK programs into a flat bytecode instruction stream, then runs them on a stack-based virtual machine. This eliminates the recursive AST-walking overhead of a tree interpreter — no per-node pattern matching, no heap pointer chasing through `Box<Expr>`, and better CPU cache locality from contiguous instruction arrays. Short-circuit `&&`/`||` and all control flow (loops, break/continue, if/else) are resolved to jump-patched offsets at compile time. The string pool interns all variable names and string constants so the VM refers to them by cheap `u32` index. **Indexed variable slots:** scalar variables are assigned `u16` slot indices at compile time and stored in a flat `Vec<Value>` — variable reads and writes are direct array indexing instead of `HashMap` lookups. Special awk variables (`NR`, `FS`, `OFS`, …) and array names remain on the HashMap path. **Stdout buffering:** print output is accumulated in a `Vec<u8>` buffer per rule execution and flushed in a single `write_all`, avoiding per-print stdout locking and reducing syscalls. **Parallel** mode shares the compiled program via **`Arc`** across rayon workers (zero-copy); each worker gets its own stack, slots, and runtime overlay.

## Still missing or partial

**Two-way pipe** (**`|&`** / **`getline … <&`**): **`sh -c`** with stdin and stdout connected (same command string for both directions). Mixing **`|`** and **`|&`** on the same command string is an error. On **Unix**, string **`==`**, **`!=`**, and relational ordering use **`strcoll`** (honors **`LC_COLLATE`** / **`LC_ALL`** from the environment). With **`-N`** / **`--use-lc-numeric`**, **`LC_NUMERIC`** is applied (`setlocale(LC_NUMERIC, "")`) and **`sprintf`** / **`printf`** (statement and function) use the locale **decimal radix** for **`%f`** / **`%e`** / **`%g`** / **`%E`** / **`%F`** / **`%G`** output; **`print`** still uses the existing numeric-to-string rules (not full POSIX **`OFMT`** on every `print` yet). Without **`-N`**, numeric formatting in **`sprintf`** uses **`.`**. Exotic **`printf`** combinations not covered above may differ from **gawk**. Many gawk-only extensions are absent. `system()` runs commands via `sh -c` (same caveat as other awks). Prefer validating critical scripts against reference `awk`/`gawk`.

## License

MIT — see `Cargo.toml`.
