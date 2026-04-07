# awkrs

Awk-style record processor in Rust (union CLI, sequential engine), created by MenkeTechnologies.

## What it does

`awkrs` runs **pattern → action** programs over input records (lines by default), similar to POSIX `awk`, GNU `gawk`, and `mawk`. The CLI accepts a **union** of common options from those implementations so scripts can pass flags through; not every extension flag changes behavior yet—see `--help`.

## Language coverage

Implemented end-to-end:

- **Rules:** `BEGIN`, `END`, empty pattern, `/regex/`, expression patterns, **range patterns** (`/a/,/b/` or `NR==1,NR==5`).
- **Statements:** `if` / `while` / `for` (C-style and `for (i in arr)`), blocks, **`print`** (with no expressions, prints **`$0`**), `break`, `continue`, **`next`**, **`exit`**, **`delete`**, **`return`** (inside functions), **`getline`** (primary input and `getline < file`).
- **Data:** fields (`$n`, `$NF`), scalars, **associative arrays** (`a[k]`), `split`, string/number values.
- **Functions:** builtins (`length`, `index`, `substr`, **`split`**, **`sprintf`**, **`printf`** (basic `%s` / `%d` / `%f` / `%%`), **`gsub`** / **`sub`** / **`match`**, `tolower` / `toupper`, `int`, `sqrt`, `rand` / `srand`, `system`, `close`), and **user-defined `function`** with parameters and locals (parameters are local; other names assign to globals, matching classic awk).
- **I/O model:** The main record loop and **`getline` with no redirection** share one **`BufReader`** on stdin or the current input file so line order matches POSIX expectations. **`exit`** sets the process status; **`END` rules still run** after `exit` from `BEGIN` or a pattern action (POSIX-style), then the process exits with the requested code.

## Multithreading

Record processing is **sequential** (correct `NR` / `FNR` / side effects). Flags **`-j` / `--threads`** and **`--read-ahead`** are accepted for CLI compatibility; the engine does not yet use a background reader thread (reserved for future work).

## Build

```bash
cargo build --release
```

`awkrs --help` / `-h` prints a **cyberpunk HUD** (ASCII banner, status box, taglines, footer) in the style of MenkeTechnologies `tp -h`. ANSI colors apply when stdout is a TTY; set `NO_COLOR` to force plain text.

## Test

```bash
cargo test
```

## Still missing or partial

`fflush`, full `printf`/`sprintf` spec, `patsplit`, multidimensional arrays, `BEGINFILE`/`ENDFILE`, coprocesses and two-way pipes, exact POSIX locale/comparison edge cases, and many gawk-only extensions. `system()` runs commands via `sh -c` (same caveat as other awks). Prefer validating critical scripts against reference `awk`/`gawk`.

## License

MIT — see `Cargo.toml`.
