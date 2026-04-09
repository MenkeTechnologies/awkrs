# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 17:07:15
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.4 ± 0.7 | 5.9 | 9.8 | 1.13 ± 0.17 |
| `awkrs (bytecode only)` | 6.6 ± 0.8 | 4.9 | 9.0 | 1.00 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 12.9 ± 1.1 | 11.0 | 18.7 | 1.73 ± 0.22 |
| `awkrs (bytecode only)` | 7.5 ± 0.7 | 6.4 | 9.3 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.5 ± 1.0 | 12.1 | 16.2 | 1.01 ± 0.10 |
| `awkrs (bytecode only)` | 13.4 ± 0.9 | 11.9 | 16.1 | 1.00 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 18.1 ± 0.8 | 16.6 | 20.2 | 1.02 ± 0.06 |
| `awkrs (bytecode only)` | 17.9 ± 0.8 | 16.8 | 20.3 | 1.00 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 5.2 ± 1.0 | 4.0 | 10.7 | 1.11 ± 0.27 |
| `awkrs (bytecode only)` | 4.7 ± 0.7 | 3.5 | 6.9 | 1.00 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 76.7 ± 6.5 | 68.6 | 104.4 | 1.37 ± 0.14 |
| `awkrs (bytecode only)` | 56.1 ± 3.5 | 50.9 | 65.5 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.4 ± 1.1 | 11.6 | 16.8 | 1.00 |
| `awkrs (bytecode only)` | 13.5 ± 0.7 | 11.5 | 15.9 | 1.00 ± 0.10 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 15.4 ± 0.9 | 13.4 | 20.5 | 1.01 ± 0.09 |
| `awkrs (bytecode only)` | 15.3 ± 0.9 | 13.5 | 17.7 | 1.00 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 17.8 ± 1.1 | 15.9 | 22.0 | 1.00 |
| `awkrs (bytecode only)` | 17.9 ± 1.1 | 15.8 | 21.5 | 1.01 ± 0.09 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 5.9 ± 0.8 | 4.7 | 8.2 | 1.03 ± 0.18 |
| `awkrs (bytecode only)` | 5.7 ± 0.6 | 4.8 | 7.5 | 1.00 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

