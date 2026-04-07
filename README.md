# awkrs

Awk-style record processor in Rust (union CLI, parallel record engine when safe), created by MenkeTechnologies.

## What it does

`awkrs` runs **pattern → action** programs over input records (lines by default), similar to POSIX `awk`, GNU `gawk`, and `mawk`. The CLI accepts a **union** of common options from those implementations so scripts can pass flags through; not every extension flag changes behavior yet—see `--help`.

## Language coverage

Implemented end-to-end:

- **Rules:** `BEGIN`, `END`, **`BEGINFILE`** / **`ENDFILE`** (gawk-style, per input file), empty pattern, `/regex/`, expression patterns, **range patterns** (`/a/,/b/` or `NR==1,NR==5`).
- **Statements:** `if` / `while` / `for` (C-style and `for (i in arr)`), blocks, **`print`** (with no expressions, prints **`$0`**), `break`, `continue`, **`next`**, **`exit`**, **`delete`**, **`return`** (inside functions), **`getline`** (primary input and `getline < file`).
- **Data:** fields (`$n`, `$NF`), scalars, **associative arrays** (`a[k]`, **`a[i,j]`** with **`SUBSEP`**), `split`, **`patsplit`** (2–3 args; optional fourth-arg `seps` array not implemented), string/number values.
- **Functions:** builtins (`length`, `index`, `substr`, **`split`**, **`sprintf`** / **`printf`** (common flags and `%s` `%d` `%i` `%u` `%o` `%x` `%X` `%f` `%e` `%E` `%g` `%G` `%c` `%%`), **`gsub`** / **`sub`** / **`match`**, `tolower` / `toupper`, `int`, `sqrt`, `rand` / `srand`, `system`, `close`, **`fflush`** (stdout / empty string only)), and **user-defined `function`** with parameters and locals (parameters are local; other names assign to globals, matching classic awk).
- **I/O model:** The main record loop and **`getline` with no redirection** share one **`BufReader`** on stdin or the current input file so line order matches POSIX expectations. **`exit`** sets the process status; **`END` rules still run** after `exit` from `BEGIN` or a pattern action (POSIX-style), then the process exits with the requested code.

## Multithreading

By default **`-j`** / **`--threads`** is set to the CPU count (`num_cpus`). When the program is **parallel-safe** (static check: no range patterns, no `exit`, no primary `getline`, no `delete`, no cross-record assignments or other mutating expressions in record rules or user functions), **records are processed in parallel** with **rayon**; `print` / `printf` output is **reordered to input order** so pipelines stay deterministic.

If the program is not parallel-safe, the engine **falls back to sequential** processing and prints a **warning** (use **`-j 1`** to force a single thread and silence the warning). **`END`** still sees only **post-`BEGIN`** global state (record-rule mutations from parallel workers are not merged into the main runtime). Flags **`--read-ahead`** are accepted for CLI compatibility; the prefetch reader thread is not used.

**Tradeoff:** Parallel mode loads the whole input file (or stdin) into memory before executing rules.

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

## Still missing or partial

**`fflush(filename)`** for user-opened output streams (only default / `fflush("")` / `fflush()` flush stdout here). **`patsplit`** without the optional fourth `seps` array. Full POSIX `printf`/`sprintf` parity (positional specifiers, dynamic width from `*`, etc.). Coprocesses and two-way pipes, exact POSIX locale/comparison edge cases, and many gawk-only extensions. `system()` runs commands via `sh -c` (same caveat as other awks). Prefer validating critical scripts against reference `awk`/`gawk`.

## License

MIT — see `Cargo.toml`.
