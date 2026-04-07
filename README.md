# awkrs

Multithreaded awk-style record processor in Rust, created by MenkeTechnologies.

## What it does

`awkrs` runs a **pattern → action** program over input records (lines by default), similar to POSIX `awk`, GNU `gawk`, and `mawk`. The CLI accepts a **union** of common options from those implementations so scripts can pass flags through; not every extension flag changes behavior yet—see `--help`.

## Multithreading

Record processing is **sequential** (awk semantics depend on `NR`, `FNR`, and side effects). Parallelism is used for **I/O**: a dedicated reader thread fills a bounded queue so the interpreter can overlap reads with execution. Tune queue depth with `--read-ahead` and reserve worker capacity with `-j` / `--threads` (used for future pools; defaults to CPU count).

## Build

```bash
cargo build --release
```

## Test

```bash
cargo test
```

## Limitations

This is a **real** lexer/parser/interpreter, but it is **not** a complete drop-in for every awk program: arrays, many builtins, `getline`, user `function`, `nextfile`, and several gawk/mawk-only semantics are incomplete or absent. Prefer validating critical scripts against reference `awk`/`gawk`.

## License

MIT — see `Cargo.toml`.
