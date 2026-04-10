# Compatibility matrix (BSD awk / mawk / gawk / awkrs)

High-level comparison only. Authoritative behavior notes and flag-by-flag gaps live in the repo [README](../README.md) (especially [§0x00 System scan](../README.md#0x00-system-scan) and [§0x03 Language coverage](../README.md#0x03-language-coverage)).

| Area | BSD awk | mawk | gawk | awkrs |
| --- | --- | --- | --- | --- |
| POSIX awk core (records, fields, patterns, `print`/`printf`, basic builtins) | Yes (implementation-defined corners exist) | Yes (fast, smaller surface) | Yes | Yes |
| GNU extensions (`BEGINFILE`/`ENDFILE`, `PROCINFO`, `SYMTAB`, `@include`, CSV mode, coprocess I/O, `/inet/`, namespaces, …) | Mostly no | Mostly no | Yes | Large subset; gaps called out in README |
| CLI: POSIX + common gawk + mawk-style flags | POSIX + vendor extras | POSIX + `-W` options | gawk’s CLI | Union accepted; some flags are no-ops or awkrs-specific diagnostics (README table) |
| Arbitrary-precision floats (`-M` / `MPFR`) | No | No | Yes (`-M`) | Yes (`-M`; JIT off in that mode) |
| Parallel record workers (`-j` / `--threads`) | No | No | No | Yes when the program passes awkrs static parallel-safety checks; else sequential with warning |
| Bytecode VM + optional Cranelift JIT | No | No | No | Yes (`AWKRS_JIT=0` forces interpreter) |
| Interactive debugger / gawk profiler output | No | No | Yes | No — `-D`/`-p` are static listing / awkrs timing summaries |

**Engines in the wild:** “BSD awk” here means typical BSD-derived `/usr/bin/awk` (e.g. macOS). Linux often ships `mawk` or `gawk` as `/usr/bin/awk`. Treat this matrix as a rough map, not a certification.
