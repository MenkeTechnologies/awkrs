# awkrs

Multithreaded awk-style record processor in Rust, created by MenkeTechnologies.

## What it does

`awkrs` runs **pattern → action** programs over input records (lines by default), similar to POSIX `awk`, GNU `gawk`, and `mawk`. The CLI accepts a **union** of common options from those implementations so scripts can pass flags through; not every extension flag changes behavior yet—see `--help`.

## Language coverage

Implemented end-to-end:

- **Rules:** `BEGIN`, `END`, empty pattern, `/regex/`, expression patterns, **range patterns** (`/a/,/b/` or `NR==1,NR==5`).
- **Statements:** `if` / `while` / `for` (C-style and `for (i in arr)`), blocks, `print`, `break`, `continue`, **`next`**, **`exit`**, **`delete`**, **`return`** (inside functions).
- **Data:** fields (`$n`, `$NF`), scalars, **associative arrays** (`a[k]`), `split`, string/number values.
- **Functions:** builtins (`length`, `index`, `substr`, **`split`**, **`sprintf`**, **`printf`** with basic `%s` / `%d` / `%f` / `%%`), and **user-defined `function`** with parameters and locals (parameters are local; other names assign to globals, matching classic awk).
- **I/O model:** `exit` terminates the process with the given status. **`exit` inside `BEGIN` does not run `END`** (differs from POSIX/gawk, which still run `END` unless you use `exit` in a specific way).

## Multithreading

Record processing is **sequential** (correct `NR` / `FNR` / side effects). A **reader thread** fills a bounded queue so execution can overlap input; use **`--read-ahead`** and **`-j` / `--threads`** for tuning (defaults to CPU count).

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```

## Still missing or partial

`getline`, `system`, `fflush`, full `printf`/`sprintf` spec, `gsub`/`sub`/`match`, `patsplit`, multidimensional arrays, `BEGINFILE`/`ENDFILE`, exact POSIX comparison rules for strings vs numbers, and many gawk-only extensions. Prefer validating critical scripts against reference `awk`/`gawk`.

## License

MIT — see `Cargo.toml`.
