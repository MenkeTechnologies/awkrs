#!/usr/bin/env python3
"""Generate POSIX-oriented parity cases (no gawk-only bitwise builtins).

Output: parity/cases_portable/{id}_portable.awk for id in [start, end] (default 2000–2999).

Safe for reference comparison with **gawk**, **mawk**, and **BSD awk** (use `nawk` on Linux
distros where /usr/bin/awk is mawk). Regenerate after edits; run:

 bash parity/run_parity.sh gawk
  bash parity/run_parity.sh mawk
  bash parity/run_parity.sh bsd"""

from __future__ import annotations

import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CASES = ROOT / "cases_portable"


def params(case_id: int) -> dict[str, int]:
    n = case_id
    return {
        "n": n,
        "a": (n * 7) % 97 + 1,
        "b": (n * 13) % 89 + 2,
        "c": (n * 3) % 83 + 3,
        "d": (n * 11) % 79 + 4,
        "m3": n % 3,
        "m5": n % 5,
        "m7": n % 7,
        "m11": n % 11,
        "m17": n % 17,
    }


# No and()/or()/xor()/lshift()/rshift() — those are gawk extensions (mawk rejects them).
TEMPLATES: list[str] = [
    'printf "%d\\n", int(({a} + {b}) * {c} / 7)',
    'printf "%d\\n", length(sprintf("p%ddq", {n}))',
    'printf "%d\\n", match("x{n}yz", /[0-9]+/)',
    'printf "%d\\n", index("alphabet", "h") + {m3}',
    'printf "%s\\n", substr("0123456789", {m5}, 4)',
    'printf "%d\\n", split("{a}:{b}:{c}", t, ":") + length(t[2])',
    'printf "%s\\n", tolower("X{n}Y")',
    'printf "%s\\n", toupper("ab{m5}c")',
    'printf "%d\\n", int(sqrt({c} * {c} + {a}))',
    'printf "%d\\n", int(log({c} + 1) * 10)',
    'printf "%.0f\\n", sin(0) + cos(0) + {m3}',
    'printf "%d\\n", (atan2(1, 1) > 0)',
    'printf "%d\\n", int(exp(log({a} + 1.0)))',
    'printf "%s\\n", sprintf("%02x", {m17} + 10)',
    '{{ _v = {m3}; printf "%d\\n", _v ? 5 : 0 }}',
    '{{ s = 0; for (j = 1; j <= {m7} + 1; j++) s += j; printf "%d\\n", s }}',
    'printf "%d\\n", ({a} < {b}) + ({b} < {c}) * 2',
    '{{ x = "a{n}b"; sub(/[0-9]+/, "Z", x); printf "%s\\n", x }}',
    '{{ x = "n{n}n"; gsub(/n/, "m", x); printf "%s\\n", x }}',
    '{{ a1[1] = {a}; a1[2] = {b}; printf "%d\\n", a1[1] + a1[2] }}',
    '{{ delete a2; a2["k"] = {c}; printf "%d\\n", a2["k"] }}',
    '{{ _s = ""; for (_i = 0; _i < {m5} + 2; _i++) _s = _s "0"; printf "%d\\n", length(_s) }}',
    '{{ u = ""; printf "%d\\n", (u == "") + 1 }}',
    'printf "%d\\n", ("ab" < "ac") + ("x" == "x")',
]


def render(case_id: int) -> str:
    p = params(case_id)
    lines: list[str] = [f"# portable:{case_id}"]
    lines.append("BEGIN {")
    n_templates = len(TEMPLATES)
    for off in range(4):
        idx = (case_id + off * 17) % n_templates
        lines.append(f"    {TEMPLATES[idx].format(**p)}")
    lines.append("}")
    return "\n".join(lines) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--start", type=int, default=2000)
    ap.add_argument("--end", type=int, default=2999)
    args = ap.parse_args()
    CASES.mkdir(parents=True, exist_ok=True)
    for case_id in range(args.start, args.end + 1):
        (CASES / f"{case_id}_portable.awk").write_text(render(case_id), encoding="utf-8")
    n = args.end - args.start + 1
    print(f"wrote {n} portable parity cases to {CASES}/<id>_portable.awk")


if __name__ == "__main__":
    main()
