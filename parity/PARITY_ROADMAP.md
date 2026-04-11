# Awk parity roadmap (awkrs)

**Goal:** Lock observable behavior to reference implementations on shared inputs: **gawk**, **mawk**, and **BSD awk** all run the **same** corpus (`parity/cases/` + `parity/cases_portable/`) — so compatibility work is driven by failing parity cases, not guesswork.

## Harness

- `parity/run_parity.sh` runs a reference **`awk -f`** and **`awkrs -f`** on the same program, then compares **combined stdout+stderr** (exact bytes, `LC_ALL=C`).
- **Modes:** `gawk` (default), `mawk`, `bsd`, or `all` (runs the three modes in sequence; exits 1 if any fail).
- **Input conventions** (paths share the stem of the `.awk` file, in any case directory):
  - **`*.in`** — stdin for both runs.
  - **`*.dat`** — one trailing input file after `-f`.
  - Neither — empty stdin (`</dev/null`).
- **Env:** `AWKRS` (default `target/release/awkrs`), `GAWK`, `MAWK`, `BSD_AWK`. The script builds release `awkrs` if missing.

**Run (repo root):**

```sh
bash parity/run_parity.sh           # gawk, full corpus
bash parity/run_parity.sh mawk      # mawk, same corpus as gawk
bash parity/run_parity.sh bsd       # BSD awk, same corpus as gawk
bash parity/run_parity.sh all
```

## Case layout

| Directory | Role |
|-----------|------|
| `parity/cases/` | **Seed** programs (`001_*.awk` …) plus **`1000_bulk.awk`–`1999_bulk.awk`**: machine corpus from `gen_parity_awk.py` (portable `pb_*` bitwise helpers, not gawk `and`/`xor`/…). |
| `parity/cases_portable/` | **`2000_portable.awk`–`2999_portable.awk`**: no gawk-only bitwise builtins; safe for **mawk** and **BSD awk**. |

**Which mode runs which files**

- **gawk, mawk, and bsd:** `parity/cases/*.awk` **and** `parity/cases_portable/*.awk` (identical lists; only the reference `awk` binary changes).

On Linux, `/usr/bin/awk` is often **mawk**; for **bsd** mode set **`BSD_AWK=nawk`** (or another BSD awk) after installing the distro package. On macOS, the harness defaults to **`/usr/bin/awk`** when `nawk` is absent.

## Generators

- **`python3 parity/gen_parity_awk.py`** — `1000_bulk.awk` … `1999_bulk.awk` (POSIX-friendly bitwise via emitted `pb_*` helpers).
- **`python3 parity/gen_parity_portable_awk.py`** — `cases_portable/2000_portable.awk` … `2999_portable.awk`.

After changing templates, regenerate and run **`bash parity/run_parity.sh all`** before committing.

## When adding cases

- **Shared machine / bulk behavior:** extend **`parity/cases/`** via **`gen_parity_awk.py`** (keep templates portable for all reference awks).
- **Portable behavior (all three references):** extend **`parity/cases_portable/`** or add a new `0xx_*.awk` seed — avoid gawk-only syntax in those files.

## PR bar

Changes that claim compatibility with **gawk**, **mawk**, or **BSD awk** should add or extend the matching parity tree (or `cargo` tests that compare to the same reference where clearer).
