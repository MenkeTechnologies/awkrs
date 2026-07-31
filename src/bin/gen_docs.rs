//! Offline generator for `docs/reference.html` — the full awkrs language
//! reference rendered as a static HTML page using the same cyberpunk styling
//! as `docs/index.html`. Run with `cargo run --bin gen-docs` before pushing to
//! GitHub Pages.
//!
//! Two corpora feed the page, split by whether an editor can put a cursor on
//! the name:
//!
//! * **`awkrs::lsp`** — `builtin_signature`, `special_doc`, `keyword_doc`.
//!   Every identifier-shaped name: the 68 builtins in
//!   `awkrs::namespace::BUILTIN_NAMES`, the 28 special globals in
//!   `SPECIAL_GLOBAL_NAMES`, and the 24 keywords in `AWK_KEYWORDS`. The
//!   generator and the LSP hover path render from the exact same strings, so
//!   the static page never drifts from what editors show on hover.
//! * **[`Entry`] tables in this file** — operators, redirection and `getline`
//!   forms, pattern forms, `printf` conversions and flags, string escapes,
//!   source directives and the inline-Rust block, `PROCINFO` keys,
//!   `sorted_in` tokens, command-line options, and environment variables.
//!   The LSP's word scanner only matches `[A-Za-z0-9_]`, so none of these are
//!   reachable by hover and none of them belong in the editor corpus.
//!
//! Chapters are explicit ordered lists (the LSP corpus is stored as `match`
//! arms, which aren't enumerable). Builtin entries additionally carry two
//! generated lines derived from the engine tables below: which execution
//! engine serves the call, and whether `--posix` / `--traditional` /
//! `--sandbox` reject it.
//!
//! The markdown → HTML converter is intentionally minimal (in-house, no crate
//! dependency): it handles what the corpus actually uses — fenced code blocks,
//! inline backticks, `**bold**`, paragraph breaks, `###` headings, and bullet
//! lists.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use awkrs::lsp::{builtin_signature, keyword_doc, special_doc};

fn main() {
    let out_path = PathBuf::from("docs/reference.html");
    let html = build_page();
    fs::write(&out_path, html).expect("write docs/reference.html");
    println!("wrote {}", out_path.display());
}

/// One reference entry that is not identifier-shaped and therefore has no home
/// in the LSP corpus: a heading, the signature line rendered in a fenced block,
/// that block's language class, and prose.
struct Entry {
    /// Heading text; also the basis of the entry anchor.
    name: &'static str,
    /// Signature / usage line shown in the fenced block.
    sig: &'static str,
    /// Fence language: `awk` for program text, `sh` for command lines.
    lang: &'static str,
    /// Prose description, written from the implementation.
    desc: &'static str,
}

/// Where a chapter's entries come from.
enum Entries {
    /// Names resolved through the shared `awkrs::lsp` lookups.
    Corpus(&'static [&'static str]),
    /// Entries defined in this file.
    Local(&'static [Entry]),
}

/// One reference chapter: a heading plus its ordered entries.
struct Chapter {
    title: &'static str,
    entries: Entries,
}

/// Builtins the **numeric-chunk bridge** lowers to a native `fusevm::Op::Awk*`,
/// paired with the op it emits. This is the default path: when a whole bytecode
/// chunk is numerically eligible, awkrs hands it to `fusevm::VM` instead of
/// running its own opcode loop. Source: the `CallBuiltin` arms of
/// `is_fusevm_eligible` and the emitter in `src/fusevm_bridge.rs`.
///
/// Offload is skipped when the JIT is off (`-s`/`--no-optimize`, `AWKRS_JIT=0`),
/// when `AWKRS_FUSEVM=0` is set, under `-M`/`--bignum` (MPFR values have no
/// `f64` slot form), and for any chunk that also touches strings, fields,
/// arrays, regexes, or I/O.
const BRIDGE_OPS: &[(&str, &str)] = &[
    ("and", "AwkAnd"),
    ("atan2", "AwkAtan2"),
    ("compl", "AwkComplJit"),
    ("cos", "AwkCos"),
    ("exp", "AwkExp"),
    ("int", "AwkInt"),
    ("log", "AwkLogJit"),
    ("lshift", "AwkLshiftJit"),
    ("mkbool", "AwkMkbool"),
    ("or", "AwkOr"),
    ("rshift", "AwkRshiftJit"),
    ("sin", "AwkSin"),
    ("sqrt", "AwkSqrtJit"),
    ("xor", "AwkXor"),
];

/// Builtins the **fusevm-native backend** lowers to a first-class
/// `fusevm::Op::Awk*`, paired with the op it emits. This is the opt-in whole-program
/// backend behind `AWKRS_FUSEVM_NATIVE=1`, distinct from the chunk bridge above:
/// it compiles the awk AST straight to a `fusevm::Chunk` rather than offloading
/// pieces of awkrs bytecode. Source: `compile_builtin_call` and
/// `compile_sub_gsub` in `src/fusevm_compile.rs`.
///
/// A builtin in neither table is served only by awkrs's own VM dispatch
/// (`exec_builtin_dispatch` in `src/vm_builtins.rs`).
const NATIVE_OPS: &[(&str, &str)] = &[
    ("atan2", "AwkAtan2"),
    ("cos", "AwkCos"),
    ("exp", "AwkExp"),
    ("gsub", "AwkGsub"),
    ("index", "AwkIndex"),
    ("int", "AwkInt"),
    ("length", "AwkLength"),
    ("log", "AwkLog"),
    ("match", "AwkMatch"),
    ("sin", "AwkSin"),
    ("sprintf", "AwkSprintf"),
    ("sqrt", "AwkSqrt"),
    ("sub", "AwkSub"),
    ("substr", "AwkSubstr"),
    ("tolower", "AwkToLower"),
    ("toupper", "AwkToUpper"),
];

/// Builtins that `-P`/`--posix` and `-c`/`--traditional` refuse to call.
/// Verbatim from `GAWK_ONLY_BUILTINS` in `src/vm_builtins.rs`.
const POSIX_REJECTED: &[&str] = &[
    "and",
    "or",
    "xor",
    "compl",
    "lshift",
    "rshift",
    "gensub",
    "patsplit",
    "mkbool",
    "mktime",
    "strftime",
    "systime",
    "isarray",
    "typeof",
    "strtonum",
    "dcgettext",
    "dcngettext",
    "bindtextdomain",
    "chdir",
    "stat",
    "statvfs",
    "fts",
    "chr",
    "ord",
    "gettimeofday",
    "getlocaltime",
    "sleep",
    "readfile",
    "readdir",
    "reada",
    "writea",
    "inplace_tmpfile",
    "inplace_commit",
    "rename",
    "revoutput",
    "revtwoway",
    "intdiv",
    "intdiv0",
];

/// Builtins that call `Runtime::require_unsandboxed_io` (or the equivalent
/// sandbox check) and therefore fail under `-S`/`--sandbox`. Source:
/// `src/gawk_extensions.rs` plus the `system` arm of `exec_builtin_dispatch`.
const SANDBOX_BLOCKED: &[&str] = &[
    "chdir",
    "fts",
    "inplace_commit",
    "inplace_tmpfile",
    "reada",
    "readdir",
    "readfile",
    "rename",
    "stat",
    "statvfs",
    "system",
    "writea",
];

/// Ordered chapters covering the whole surface.
const CHAPTERS: &[Chapter] = &[
    Chapter {
        title: "String Functions",
        entries: Entries::Corpus(&[
            "length", "substr", "index", "split", "sub", "gsub", "gensub", "match", "sprintf",
            "tolower", "toupper",
        ]),
    },
    Chapter {
        title: "Arithmetic Functions",
        entries: Entries::Corpus(&[
            "sin", "cos", "atan2", "exp", "log", "sqrt", "int", "intdiv", "intdiv0", "strtonum",
            "rand", "srand",
        ]),
    },
    Chapter {
        title: "I/O and General Functions",
        entries: Entries::Corpus(&["print", "printf", "getline", "close", "fflush", "system"]),
    },
    Chapter {
        title: "Time Functions",
        entries: Entries::Corpus(&[
            "systime",
            "strftime",
            "mktime",
            "gettimeofday",
            "getlocaltime",
            "sleep",
        ]),
    },
    Chapter {
        title: "Bitwise Functions",
        entries: Entries::Corpus(&["and", "or", "xor", "compl", "lshift", "rshift"]),
    },
    Chapter {
        title: "Array and Type Functions",
        entries: Entries::Corpus(&["typeof", "isarray", "mkbool", "patsplit", "asort", "asorti"]),
    },
    Chapter {
        title: "Character and Text Functions",
        entries: Entries::Corpus(&["chr", "ord", "revoutput", "revtwoway"]),
    },
    Chapter {
        title: "File and Directory Functions",
        entries: Entries::Corpus(&[
            "stat",
            "statvfs",
            "fts",
            "readdir",
            "readfile",
            "rename",
            "chdir",
            "inplace_tmpfile",
            "inplace_commit",
            "writea",
            "reada",
        ]),
    },
    Chapter {
        title: "Localization Functions",
        entries: Entries::Corpus(&["bindtextdomain", "dcgettext", "dcngettext"]),
    },
    Chapter {
        title: "The Intercept Engine",
        entries: Entries::Local(INTERCEPT_ENGINE),
    },
    Chapter {
        title: "Intercept Functions",
        entries: Entries::Corpus(&[
            "intercept",
            "intercept_proceed",
            "intercept_list",
            "intercept_remove",
            "intercept_clear",
        ]),
    },
    Chapter {
        title: "Special Variables",
        entries: Entries::Corpus(&[
            "NR",
            "NF",
            "FNR",
            "FILENAME",
            "FS",
            "OFS",
            "ORS",
            "RS",
            "RT",
            "SUBSEP",
            "RSTART",
            "RLENGTH",
            "CONVFMT",
            "OFMT",
            "FPAT",
            "FIELDWIDTHS",
            "IGNORECASE",
            "ARGC",
            "ARGV",
            "ARGIND",
            "ENVIRON",
            "ERRNO",
            "PROCINFO",
            "SYMTAB",
            "FUNCTAB",
            "BINMODE",
            "LINT",
            "TEXTDOMAIN",
        ]),
    },
    Chapter {
        title: "Keywords and Control Flow",
        entries: Entries::Corpus(&[
            "BEGIN",
            "END",
            "BEGINFILE",
            "ENDFILE",
            "function",
            "return",
            "if",
            "else",
            "while",
            "do",
            "for",
            "in",
            "switch",
            "case",
            "default",
            "break",
            "continue",
            "next",
            "nextfile",
            "exit",
            "delete",
        ]),
    },
    Chapter {
        title: "Pattern Forms",
        entries: Entries::Local(PATTERN_FORMS),
    },
    Chapter {
        title: "Operators",
        entries: Entries::Local(OPERATORS),
    },
    Chapter {
        title: "Redirection and getline Forms",
        entries: Entries::Local(REDIRECTION_FORMS),
    },
    Chapter {
        title: "printf Conversions",
        entries: Entries::Local(PRINTF_CONVERSIONS),
    },
    Chapter {
        title: "printf Flags, Width, and Precision",
        entries: Entries::Local(PRINTF_MODIFIERS),
    },
    Chapter {
        title: "String Escape Sequences",
        entries: Entries::Local(ESCAPES),
    },
    Chapter {
        title: "Source Directives and Inline Rust",
        entries: Entries::Local(DIRECTIVES),
    },
    Chapter {
        title: "PROCINFO Keys",
        entries: Entries::Local(PROCINFO_KEYS),
    },
    Chapter {
        title: "Array Traversal Order",
        entries: Entries::Local(SORTED_IN),
    },
    Chapter {
        title: "Command-Line Options",
        entries: Entries::Local(CLI_OPTIONS),
    },
    Chapter {
        title: "Environment Variables",
        entries: Entries::Local(ENV_VARS),
    },
];

/// Resolve a corpus name to its hover-identical markdown, trying builtins
/// first, then special variables, then keywords. Returns `None` if the name
/// is undocumented (so the generator can warn rather than emit a blank entry).
///
/// Builtins pick up two generated trailer lines that hover deliberately omits:
/// which engine executes the call, and which compatibility flags reject it.
fn doc_markdown(name: &str) -> Option<String> {
    if let Some((sig, desc)) = builtin_signature(name) {
        let mut md = format!("```awk\n{sig}\n```\n\n{desc}\n\n{}", engine_line(name));
        if let Some(line) = availability_line(name) {
            md.push_str("\n\n");
            md.push_str(&line);
        }
        Some(md)
    } else if let Some(desc) = special_doc(name) {
        Some(format!("**`{name}`** — special variable\n\n{desc}"))
    } else {
        keyword_doc(name).map(|desc| format!("**`{name}`** — keyword\n\n{desc}"))
    }
}

/// The per-builtin engine note. awkrs can reach fusevm by two independent
/// routes — the default numeric-chunk bridge and the opt-in whole-program
/// backend — and a builtin may be lowered by one, both, or neither. That is not
/// guessable from a signature, so state it per entry rather than once in a
/// preface.
fn engine_line(name: &str) -> String {
    let bridge = BRIDGE_OPS.iter().find(|(n, _)| *n == name).map(|(_, o)| *o);
    let native = NATIVE_OPS.iter().find(|(n, _)| *n == name).map(|(_, o)| *o);
    let mut line = String::from("**Engine.** Dispatched by awkrs's own VM. ");
    match (bridge, native) {
        (Some(b), Some(n)) if b == n => line.push_str(&format!(
            "Also lowered to the native `fusevm::Op::{b}` on both fusevm routes — the \
             default numeric-chunk offload and the `AWKRS_FUSEVM_NATIVE=1` backend."
        )),
        (Some(b), Some(n)) => line.push_str(&format!(
            "Also lowered to native fusevm ops on both routes: `fusevm::Op::{b}` by the \
             default numeric-chunk offload, `fusevm::Op::{n}` by the \
             `AWKRS_FUSEVM_NATIVE=1` backend."
        )),
        (Some(b), None) => line.push_str(&format!(
            "Also lowered to the native `fusevm::Op::{b}` when the default numeric-chunk \
             offload takes the surrounding chunk. The `AWKRS_FUSEVM_NATIVE=1` backend has \
             no op for it and refuses to compile a program that calls it."
        )),
        (None, Some(n)) => line.push_str(&format!(
            "Lowered to the native `fusevm::Op::{n}` by the `AWKRS_FUSEVM_NATIVE=1` \
             backend. The default numeric-chunk offload does not admit it, so an \
             ordinary run always executes the VM path."
        )),
        (None, None) => line.push_str(
            "Neither fusevm route lowers it: the numeric-chunk offload treats the chunk as \
             ineligible, and the `AWKRS_FUSEVM_NATIVE=1` backend refuses to compile a \
             program that calls it rather than lowering it to something approximate.",
        ),
    }
    line
}

/// The per-builtin compatibility note: which flags refuse the call. Omitted
/// when no flag touches it, so the line only ever appears where it is news.
fn availability_line(name: &str) -> Option<String> {
    let posix = POSIX_REJECTED.contains(&name);
    let sandbox = SANDBOX_BLOCKED.contains(&name);
    match (posix, sandbox) {
        (false, false) => None,
        (true, false) => Some(
            "**Restrictions.** Rejected as a non-POSIX extension under `-P`/`--posix` \
             and `-c`/`--traditional`."
                .to_string(),
        ),
        (false, true) => Some("**Restrictions.** Blocked under `-S`/`--sandbox`.".to_string()),
        (true, true) => Some(
            "**Restrictions.** Rejected as a non-POSIX extension under `-P`/`--posix` \
             and `-c`/`--traditional`, and blocked under `-S`/`--sandbox`."
                .to_string(),
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Local corpora. Everything below is reference-only: not identifier-shaped,
// so unreachable by the LSP's `[A-Za-z0-9_]` word scanner. Each description is
// written from the implementation named in its chapter's source comment.
// ─────────────────────────────────────────────────────────────────────────

/// The aspect-oriented advice machinery. There is no POSIX awk or gawk
/// counterpart; the join point is the user-function call and the semantics are
/// ported from zshrs. Source: `src/intercepts.rs` and `run_user_intercepts` in
/// `src/vm.rs`.
const INTERCEPT_ENGINE: &[Entry] = &[
    Entry {
        name: "before advice",
        sig: r#"intercept("before", pattern, code)"#,
        lang: "awk",
        desc: "Runs `code` before every matching user-function call, then lets normal dispatch \
               run the original. Every matching *before* advice fires, in registration order. If \
               only *before* advice matched, the call proceeds untouched — the advice cannot \
               change the arguments or the result.",
    },
    Entry {
        name: "after advice",
        sig: r#"intercept("after", pattern, code)"#,
        lang: "awk",
        desc: "Runs `code` after the original returns. The presence of *after* advice makes the \
               intercept machinery call the original itself, so `INTERCEPT_MS` / `INTERCEPT_US` \
               are populated with the measured duration before the advice runs. The original's \
               return value reaches the caller unchanged.",
    },
    Entry {
        name: "around advice",
        sig: r#"intercept("around", pattern, code)"#,
        lang: "awk",
        desc: "Wraps the call. The original does not run unless `code` calls \
               `intercept_proceed()`. At most one *around* advice is honored per call — the \
               first match wins. The value the caller receives is whatever `intercept_proceed()` \
               captured, or the awk empty value when the advice never proceeded; the advice's own \
               `return` value is discarded, so *around* advice can suppress a call but cannot \
               rewrite its result.",
    },
    Entry {
        name: "intercept patterns",
        sig: r#"intercept(kind, "draw_*", code)"#,
        lang: "awk",
        desc: "A pattern matches by exact function name, or as a shell-style glob (`*` for any \
               run including empty, `?` for one character) against both the bare function name \
               and the `\"name arg1 arg2\"` call string. `\"*\"` and `\"all\"` match every call. \
               Any other character, including `[`, matches literally — there are no bracket \
               expressions.",
    },
    Entry {
        name: "INTERCEPT_NAME",
        sig: "INTERCEPT_NAME",
        lang: "awk",
        desc: "Global exposed to advice for the span of an intercepted call: the name of the \
               function being intercepted. Removed again when the call completes.",
    },
    Entry {
        name: "INTERCEPT_ARGS",
        sig: "INTERCEPT_ARGS",
        lang: "awk",
        desc: "The call's arguments joined with single spaces, in their string forms. A lossy \
               view — use it for logging and matching, not for reconstructing values.",
    },
    Entry {
        name: "INTERCEPT_CMD",
        sig: "INTERCEPT_CMD",
        lang: "awk",
        desc: "`INTERCEPT_NAME` and `INTERCEPT_ARGS` joined by a space, or just the name when the \
               call had no arguments. This is the string glob patterns are matched against.",
    },
    Entry {
        name: "INTERCEPT_MS",
        sig: "INTERCEPT_MS",
        lang: "awk",
        desc: "Wall-clock duration of the intercepted call in milliseconds, formatted to three \
               decimals. Set only before *after* advice runs, so *before* and *around* advice \
               never see it.",
    },
    Entry {
        name: "INTERCEPT_US",
        sig: "INTERCEPT_US",
        lang: "awk",
        desc: "The same measurement in whole microseconds. Set alongside `INTERCEPT_MS`, with the \
               same *after*-advice-only visibility.",
    },
];

/// Rule shapes accepted at the top level. Source: `Pattern` in `src/ast.rs` and
/// the rule-parsing path in `src/parser.rs`.
const PATTERN_FORMS: &[Entry] = &[
    Entry {
        name: "pattern { action }",
        sig: "pattern { action }",
        lang: "awk",
        desc: "The rule form. For each input record, the pattern is evaluated and the action runs \
               when it is true. A program is a sequence of these plus function definitions; there \
               are no bare top-level statements.",
    },
    Entry {
        name: "{ action }",
        sig: "{ action }",
        lang: "awk",
        desc: "Empty pattern — the action runs for every input record.",
    },
    Entry {
        name: "pattern (no action)",
        sig: "/error/",
        lang: "awk",
        desc: "A rule with no action block prints `$0` whenever the pattern is true. `awkrs \
               '/error/' log` is the grep-shaped form.",
    },
    Entry {
        name: "/regexp/",
        sig: "/regexp/ { action }",
        lang: "awk",
        desc: "Regexp pattern: true when the ERE matches `$0`. Equivalent to `$0 ~ /regexp/`, and \
               subject to `IGNORECASE` the same way.",
    },
    Entry {
        name: "expression pattern",
        sig: "NF > 3 && $1 != \"#\" { action }",
        lang: "awk",
        desc: "Any expression works as a pattern; the action runs when the expression is true by \
               awk's truth rules (non-zero number, or non-empty string for values that are not \
               numeric strings).",
    },
    Entry {
        name: "range pattern",
        sig: "pattern1, pattern2 { action }",
        lang: "awk",
        desc: "Inclusive range: the rule turns on at the record where `pattern1` is true and off \
               after the record where `pattern2` is true, both records included. Either side may \
               be a regexp or an expression.",
    },
];

/// Operators, loosest to tightest by the precedence chain in `src/parser.rs`
/// (`parse_assign` → `parse_cond` → `parse_or` → `parse_and` → `parse_cmp` →
/// `parse_concat` → `parse_additive` → `parse_multiplicative` → `parse_unary`
/// → `parse_power` → postfix → primary). Semantics from `BinOp` evaluation in
/// `src/runtime.rs` and `src/vm.rs`.
const OPERATORS: &[Entry] = &[
    Entry {
        name: "=",
        sig: "lvalue = expr",
        lang: "awk",
        desc: "Assignment; right-associative and the loosest-binding operator, so `a = b = 1` \
               assigns 1 to both. The target may be a variable, a field, or an array element. The \
               expression's value is the assigned value.",
    },
    Entry {
        name: "+= -= *= /= %=",
        sig: "lvalue += expr",
        lang: "awk",
        desc: "Compound assignment: read the target, apply the binary operator, store the result. \
               `/=` and `%=` raise the same fatal division-by-zero error as their binary forms.",
    },
    Entry {
        name: "^= **=",
        sig: "lvalue ^= expr",
        lang: "awk",
        desc: "Compound exponentiation. `**=` is accepted as a synonym for `^=`; the lexer emits \
               one token for either spelling.",
    },
    Entry {
        name: "?:",
        sig: "cond ? then : else",
        lang: "awk",
        desc: "Conditional expression; only the selected arm is evaluated. Binds tighter than \
               assignment and looser than `||`. Newlines directly after `?` or `:` are treated as \
               whitespace, so a ternary may be split across lines. The else arm is parsed as an \
               assignment expression, so `c ? x = 1 : x = 2` parses.",
    },
    Entry {
        name: "||",
        sig: "expr || expr",
        lang: "awk",
        desc:
            "Logical or; short-circuits, yielding 1 or 0. A newline after `||` is whitespace, so \
               a condition may be continued on the next line without a backslash.",
    },
    Entry {
        name: "&&",
        sig: "expr && expr",
        lang: "awk",
        desc: "Logical and; short-circuits, yielding 1 or 0, and continues across a newline the \
               same way `||` does. Binds tighter than `||`.",
    },
    Entry {
        name: "in",
        sig: "key in array",
        lang: "awk",
        desc: "Membership test: 1 when `array[key]` exists, else 0. Unlike a plain subscript \
               reference it does not create the element. Chains left to right at the same \
               precedence level as the comparison operators.",
    },
    Entry {
        name: "(i, j) in array",
        sig: "(i, j) in array",
        lang: "awk",
        desc: "Membership test for a multidimensional subscript: the index list is joined with \
               `SUBSEP` and looked up as a single key. The parentheses are required — they are \
               what distinguishes the form from a comma expression.",
    },
    Entry {
        name: "== !=",
        sig: "expr == expr",
        lang: "awk",
        desc: "Equality and inequality. Both operands numeric (or numeric strings from input) \
               compare numerically, so `$1 == 10` matches the field text `10.0`; a string literal \
               operand forces a string comparison, which is why `\"10\" == 10` and `10 == \"10\"` \
               can differ from a field comparison.",
    },
    Entry {
        name: "< <= > >=",
        sig: "expr < expr",
        lang: "awk",
        desc: "Relational comparison, with the same numeric-versus-string rule as `==`. Inside a \
               `print` or `printf` argument list a bare `>` is parsed as an output redirection, \
               not a comparison — parenthesize the comparison to disambiguate.",
    },
    Entry {
        name: "~",
        sig: "expr ~ /regexp/",
        lang: "awk",
        desc:
            "Regexp match: 1 when the right operand, taken as an ERE, matches the left operand's \
               string form. The right side may be a `/re/` literal, an `@/re/` typed regexp, or \
               any string-valued expression used as a dynamic regexp.",
    },
    Entry {
        name: "!~",
        sig: "expr !~ /regexp/",
        lang: "awk",
        desc: "Negated regexp match — 1 when `~` would yield 0.",
    },
    Entry {
        name: "concatenation",
        sig: "expr expr",
        lang: "awk",
        desc:
            "Juxtaposition concatenates the operands' string forms; there is no operator symbol. \
               It binds tighter than the comparisons and looser than `+`/`-`, which is why \
               `print 1 \" \" 2+3` prints `1 5`.",
    },
    Entry {
        name: "+ -",
        sig: "expr + expr",
        lang: "awk",
        desc: "Addition and subtraction on the operands' numeric values. Under `-M`/`--bignum` \
               these evaluate at MPFR precision instead of `f64`.",
    },
    Entry {
        name: "*",
        sig: "expr * expr",
        lang: "awk",
        desc: "Multiplication, numeric.",
    },
    Entry {
        name: "/",
        sig: "expr / expr",
        lang: "awk",
        desc: "Division. A zero divisor is a fatal runtime error (`division by zero attempted`), \
               not an infinity — on the fusevm path this is why awkrs lowers to `Op::AwkDiv` \
               rather than fusevm's shared `Op::Div`, which yields an undefined value instead.",
    },
    Entry {
        name: "%",
        sig: "expr % expr",
        lang: "awk",
        desc: "Remainder, taking the sign of the left operand (`-7 % 3` is `-1`). A zero divisor \
               is fatal, with the error naming `%`.",
    },
    Entry {
        name: "unary - +",
        sig: "-expr",
        lang: "awk",
        desc: "Numeric negation and the no-op unary plus, which forces a numeric coercion. Both \
               bind looser than `^`, so `-2^2` is `-4`.",
    },
    Entry {
        name: "!",
        sig: "!expr",
        lang: "awk",
        desc: "Logical negation: 1 when the operand is false by awk's truth rules, else 0.",
    },
    Entry {
        name: "^ **",
        sig: "expr ^ expr",
        lang: "awk",
        desc: "Exponentiation, right-associative — `2^3^2` is `2^(3^2)`, or 512. `**` is accepted \
               as a synonym. Postfix `++`/`--` are applied to the base before the exponent, so \
               `x++^2` squares the pre-increment value.",
    },
    Entry {
        name: "++",
        sig: "++lvalue    lvalue++",
        lang: "awk",
        desc: "Increment by one. The prefix form yields the new value, the postfix form the old \
               one. Valid on variables, fields, and array elements.",
    },
    Entry {
        name: "--",
        sig: "--lvalue    lvalue--",
        lang: "awk",
        desc: "Decrement by one, with the same prefix/postfix distinction as `++`.",
    },
    Entry {
        name: "$",
        sig: "$expr",
        lang: "awk",
        desc: "Field reference. `$0` is the whole record, `$1`…`$NF` the fields. Assigning to a \
               field past `NF` extends the record and rebuilds `$0` with `OFS`; assigning to `$0` \
               re-splits the record with the active field rule.",
    },
    Entry {
        name: "( )",
        sig: "(expr)",
        lang: "awk",
        desc: "Grouping. A `(` immediately after an identifier with no intervening space is \
               lexed as a function call instead — POSIX awk's rule that `name(arg)` calls and \
               `name (arg)` concatenates, which awkrs implements with a distinct token.",
    },
];

/// Output redirections, `getline` forms, and the network pseudo-paths. Source:
/// `PrintRedir` / `GetlineRedir` in `src/ast.rs`, the redirect handling in
/// `src/runtime.rs`, and `parse_inet_tcp` / `parse_inet_udp`.
const REDIRECTION_FORMS: &[Entry] = &[
    Entry {
        name: "print > file",
        sig: "print expr-list > file",
        lang: "awk",
        desc: "Write to `file`, truncating it the first time the program opens it and appending \
               on every later write. Repeated `>` to the same name within one run does not \
               re-truncate; `close(file)` before the next write does.",
    },
    Entry {
        name: "print >> file",
        sig: "print expr-list >> file",
        lang: "awk",
        desc: "Same as `>` except the first open appends instead of truncating.",
    },
    Entry {
        name: "print | command",
        sig: "print expr-list | command",
        lang: "awk",
        desc: "Run `command` through `sh -c` and write to its standard input. The pipe stays open \
               until `close(command)` or program exit; the command string is the handle, so two \
               redirections with the same text share one subprocess.",
    },
    Entry {
        name: "print |& command",
        sig: "print expr-list |& command",
        lang: "awk",
        desc: "Two-way pipe (coprocess): the same `sh -c` command model, but both its standard \
               input and standard output are connected. Read the other half back with `getline \
               <& command`. Deadlocks are the caller's problem — flush or `close(command, \"to\")` \
               before reading.",
    },
    Entry {
        name: "getline",
        sig: "getline",
        lang: "awk",
        desc: "Read the next record from the main input into `$0`, updating `NF`, `NR`, and \
               `FNR`. Returns 1 on a record, 0 at end of input, -1 on error.",
    },
    Entry {
        name: "getline var",
        sig: "getline var",
        lang: "awk",
        desc: "Read the next main-input record into `var`, leaving `$0` and `NF` alone; `NR` and \
               `FNR` still advance. Same 1 / 0 / -1 return protocol.",
    },
    Entry {
        name: "getline < file",
        sig: "getline < file",
        lang: "awk",
        desc: "Read the next record from `file` into `$0`, updating `NF` but not `NR` or `FNR`. \
               Returns -1 when the file cannot be opened, which is how a missing input is \
               distinguished from an empty one (0).",
    },
    Entry {
        name: "getline var < file",
        sig: "getline var < file",
        lang: "awk",
        desc: "Read from `file` into `var`, touching neither the fields nor the record counters. \
               The plainest form: no side effects beyond `var` and `ERRNO`.",
    },
    Entry {
        name: "command | getline",
        sig: "command | getline",
        lang: "awk",
        desc: "Run `command` through `sh -c` and read its next output line into `$0`, updating \
               `NF` and `NR`. Loop with `while ((cmd | getline) > 0)` and finish with \
               `close(cmd)`.",
    },
    Entry {
        name: "command | getline var",
        sig: "command | getline var",
        lang: "awk",
        desc: "Read the command's next output line into `var`; `NR` advances, the fields do not \
               change.",
    },
    Entry {
        name: "getline var <& command",
        sig: "getline var <& command",
        lang: "awk",
        desc: "Read from the output half of the coprocess started by `|&` with the same command \
               string. The `<&` spelling is a distinct token, so it is never confused with `<` \
               followed by `&`.",
    },
    Entry {
        name: "/inet/tcp/lport/host/rport",
        sig: "print req |& \"/inet/tcp/0/example.com/80\"",
        lang: "awk",
        desc: "A redirection target of this shape opens a TCP connection to `host:rport` instead \
               of a file or a subprocess. A local port of 0 means an ephemeral client port; any \
               other value is bound before connecting. Malformed paths — wrong field count, \
               unparseable ports — are a runtime error, not a silent file open.",
    },
    Entry {
        name: "/inet/udp/lport/host/rport",
        sig: "print msg > \"/inet/udp/0/198.51.100.7/514\"",
        lang: "awk",
        desc: "The same path grammar over a connected UDP socket: writes send datagrams and reads \
               receive them. An `/inet/` path that is neither `tcp` nor `udp` is rejected with an \
               error naming the two supported forms.",
    },
];

/// `printf` / `sprintf` conversion characters. Source: `is_known_conv` and
/// `format_one` in `src/format.rs`.
const PRINTF_CONVERSIONS: &[Entry] = &[
    Entry {
        name: "%d, %i",
        sig: r#"printf "%d\n", expr"#,
        lang: "awk",
        desc: "Signed decimal integer; the value is truncated toward zero. Values outside `i64` \
               fall back to a wider formatting path rather than wrapping, and under \
               `-M`/`--bignum` the integer is taken from the MPFR value.",
    },
    Entry {
        name: "%u",
        sig: r#"printf "%u\n", expr"#,
        lang: "awk",
        desc: "Unsigned decimal integer.",
    },
    Entry {
        name: "%o",
        sig: r#"printf "%o\n", expr"#,
        lang: "awk",
        desc: "Unsigned octal. With the `#` flag the output carries a leading `0`.",
    },
    Entry {
        name: "%x, %X",
        sig: r#"printf "%x %X\n", expr, expr"#,
        lang: "awk",
        desc: "Unsigned hexadecimal, lower- and upper-case. With the `#` flag the output is \
               prefixed `0x` or `0X`.",
    },
    Entry {
        name: "%a, %A",
        sig: r#"printf "%a\n", expr"#,
        lang: "awk",
        desc: "C99 hexadecimal floating point — `1.5` prints as `0x1.8p+0` (`%a`) or `0X1.8P+0` \
               (`%A`). An exact, round-trippable rendering of the underlying double.",
    },
    Entry {
        name: "%f, %F",
        sig: r#"printf "%.2f\n", expr"#,
        lang: "awk",
        desc: "Fixed-point decimal, six fraction digits by default.",
    },
    Entry {
        name: "%e, %E",
        sig: r#"printf "%e\n", expr"#,
        lang: "awk",
        desc: "Scientific notation with six fraction digits by default; `%E` uses `E` for the \
               exponent marker.",
    },
    Entry {
        name: "%g, %G",
        sig: r#"printf "%g\n", expr"#,
        lang: "awk",
        desc: "Shorter of `%e` and `%f` for the value, with trailing zeros removed unless the `#` \
               flag is given. This is the conversion `CONVFMT` and `OFMT` default to (`%.6g`).",
    },
    Entry {
        name: "%c",
        sig: r#"printf "%c\n", expr"#,
        lang: "awk",
        desc: "A single character: the first character of a string argument, or the character for \
               a numeric argument's code point. Padded with spaces even under the `0` flag, \
               except in `--traditional` mode where the BSD awk zero-padding quirk is honored.",
    },
    Entry {
        name: "%s",
        sig: r#"printf "%.3s\n", expr"#,
        lang: "awk",
        desc: "String. A precision truncates to that many characters. Like `%c`, the `0` flag \
               does not zero-pad strings except under `--traditional`.",
    },
    Entry {
        name: "%%",
        sig: r#"printf "100%%\n""#,
        lang: "awk",
        desc: "A literal percent sign. Consumes no argument.",
    },
    Entry {
        name: "unknown conversions",
        sig: r#"printf "[%q][%s]\n", "x""#,
        lang: "awk",
        desc: "A conversion character outside the set above is emitted literally as `%q` and \
               consumes no argument, so the following `%s` still receives the argument it was \
               written for. The example prints `[%q][x]`.",
    },
];

/// `printf` flags, width, precision, and argument selection. Source:
/// `parse_conversion_rest` in `src/format.rs`.
const PRINTF_MODIFIERS: &[Entry] = &[
    Entry {
        name: "- (left justify)",
        sig: r#"printf "%-8s|\n", s"#,
        lang: "awk",
        desc: "Left-justify within the field width instead of right-justifying.",
    },
    Entry {
        name: "+ (force sign)",
        sig: r#"printf "%+d\n", n"#,
        lang: "awk",
        desc: "Always print a sign on a numeric conversion, `+` for non-negative values.",
    },
    Entry {
        name: "space (sign placeholder)",
        sig: r#"printf "% d\n", n"#,
        lang: "awk",
        desc: "Print a leading space where a non-negative value's sign would go, so positive and \
               negative values align. Ignored when `+` is also given.",
    },
    Entry {
        name: "# (alternate form)",
        sig: r#"printf "%#x %#o\n", n, n"#,
        lang: "awk",
        desc: "Alternate form: `0x`/`0X` prefix for `%x`/`%X`, a leading `0` for `%o`, and \
               retained trailing zeros for `%g`/`%G`.",
    },
    Entry {
        name: "' (thousands grouping)",
        sig: r#"printf "%'d\n", 1234567"#,
        lang: "awk",
        desc: "Group the integer part with the locale's thousands separator. Under `-N`/`--use-lc-numeric` \
               the separator comes from `localeconv()`; when the locale supplies none, the flag is \
               a no-op rather than an error.",
    },
    Entry {
        name: "0 (zero pad)",
        sig: r#"printf "%05d\n", n"#,
        lang: "awk",
        desc: "Pad numeric conversions with zeros rather than spaces. The zeros go between the \
               sign and the digits, so `%05d` of -42 is `-0042`. Ignored for `%s` and `%c` \
               outside `--traditional`.",
    },
    Entry {
        name: "field width",
        sig: r#"printf "%8s|\n", s"#,
        lang: "awk",
        desc: "Minimum field width. Widths are capped at 100000 characters — beyond that the \
               request is clamped instead of attempting the allocation.",
    },
    Entry {
        name: "* (width from argument)",
        sig: r#"printf "%*d|\n", 6, 42"#,
        lang: "awk",
        desc: "Take the field width from the next argument. A negative width means left-justify, \
               exactly as if `-` had been written.",
    },
    Entry {
        name: ".precision",
        sig: r#"printf "%.3f %.2s\n", x, s"#,
        lang: "awk",
        desc: "Fraction digits for the floating conversions, significant digits for `%g`, maximum \
               characters for `%s`. A bare `.` with no digits means precision zero.",
    },
    Entry {
        name: ".* (precision from argument)",
        sig: r#"printf "%.*f\n", 3, x"#,
        lang: "awk",
        desc: "Take the precision from the next argument; a negative value is treated as zero.",
    },
    Entry {
        name: "N$ (positional argument)",
        sig: r#"printf "%2$s %1$s\n", "a", "b""#,
        lang: "awk",
        desc: "Select the Nth argument explicitly instead of consuming the next one — the example \
               prints `b a`. Useful when a translated format string needs a different argument \
               order than the original.",
    },
    Entry {
        name: "h, l, L (length modifiers)",
        sig: r#"printf "%ld\n", n"#,
        lang: "awk",
        desc: "Accepted and skipped. awk has one numeric type, so the C length modifiers carry no \
               information; they are consumed so that formats copied from C source still work.",
    },
];

/// String-literal escape sequences. Source: the string-lexing arm of
/// `src/lexer.rs`.
const ESCAPES: &[Entry] = &[
    Entry {
        name: r"\n",
        sig: r#""line\n""#,
        lang: "awk",
        desc: "Newline (0x0A).",
    },
    Entry {
        name: r"\t",
        sig: r#""a\tb""#,
        lang: "awk",
        desc: "Horizontal tab (0x09).",
    },
    Entry {
        name: r"\r",
        sig: r#""a\rb""#,
        lang: "awk",
        desc: "Carriage return (0x0D).",
    },
    Entry {
        name: r"\a",
        sig: r#""\a""#,
        lang: "awk",
        desc: "Alert / bell (0x07).",
    },
    Entry {
        name: r"\b",
        sig: r#""\b""#,
        lang: "awk",
        desc: "Backspace (0x08).",
    },
    Entry {
        name: r"\f",
        sig: r#""\f""#,
        lang: "awk",
        desc: "Form feed (0x0C).",
    },
    Entry {
        name: r"\v",
        sig: r#""\v""#,
        lang: "awk",
        desc: "Vertical tab (0x0B).",
    },
    Entry {
        name: r"\\",
        sig: r#""C:\\path""#,
        lang: "awk",
        desc: "A literal backslash.",
    },
    Entry {
        name: r#"\""#,
        sig: r#""say \"hi\"""#,
        lang: "awk",
        desc: "A literal double quote inside a string literal.",
    },
    Entry {
        name: r"\/",
        sig: r#""a\/b""#,
        lang: "awk",
        desc: "A literal forward slash. Redundant in a string but accepted, so a regexp copied \
               into a string literal keeps working.",
    },
    Entry {
        name: r"\xHH",
        sig: r#""\x41""#,
        lang: "awk",
        desc: "One or two hexadecimal digits as a code point — `\\x41` is `A`. With no hex digit \
               following, the `\\x` is kept as a literal `x`.",
    },
    Entry {
        name: r"\NNN",
        sig: r#""\101""#,
        lang: "awk",
        desc: "One to three octal digits as a byte value — `\\101` is `A`. Values above 0xFF are \
               masked to a single byte.",
    },
    Entry {
        name: r"\c (unrecognized)",
        sig: r#""\q""#,
        lang: "awk",
        desc:
            "An escape that is not in this table drops the backslash and keeps the character, so \
               `\"\\q\"` is `q`. gawk warns about this under `--lint`; awkrs accepts it silently.",
    },
];

/// Source-level directives handled before parsing, plus the two `@` expression
/// forms and the inline-Rust block. Source: `src/source_expand.rs`,
/// `src/namespace.rs`, the `Token::At` arm of `src/parser.rs`, and
/// `src/rust_ffi.rs`.
const DIRECTIVES: &[Entry] = &[
    Entry {
        name: "@include",
        sig: r#"@include "lib.awk""#,
        lang: "awk",
        desc: "Splice another source file in at this point, before lexing. Includes are resolved \
               once per path — a file already pulled in is skipped rather than duplicated, so a \
               diamond of includes does not redefine its functions.",
    },
    Entry {
        name: "@load",
        sig: r#"@load "filefuncs""#,
        lang: "awk",
        desc: "gawk's dynamic-extension directive. awkrs implements gawk's bundled modules — \
               `filefuncs`, `readdir`, `time`, `inplace`, `ordchr`, `readfile`, `revoutput`, \
               `revtwoway`, `rwarray`, `intdiv` — natively in Rust, so loading one of those names \
               (with or without a `.so` suffix or a directory prefix) is accepted and ignored; \
               nothing is `dlopen`ed. Any other name is treated as an `@include` of that `.awk` \
               file.",
    },
    Entry {
        name: "@namespace",
        sig: r#"@namespace "util""#,
        lang: "awk",
        desc: "Set the default namespace for the rest of the file: unqualified identifiers are \
               rewritten to `util::name` in the AST. Builtin names, the special globals, and \
               function-local names are never prefixed, and a name that already contains `::` is \
               left alone.",
    },
    Entry {
        name: "@/regexp/",
        sig: "r = @/^err/",
        lang: "awk",
        desc: "A typed regexp constant — a first-class value that can be assigned, passed, and \
               used on the right of `~`. `typeof()` reports `\"regexp\"`, which is what \
               distinguishes it from an ordinary string used as a dynamic regexp.",
    },
    Entry {
        name: "@expr(args)",
        sig: "f = \"handler\"; @f(x)",
        lang: "awk",
        desc: "Indirect function call: the callee name comes from the value of `expr`. The callee \
               may be a variable, an array element (`@a[k](…)`), a field (`@$1(…)`), or a \
               parenthesized expression. Parsed so that the argument list belongs to the indirect \
               call, not to the callee expression.",
    },
    Entry {
        name: "rust { … }",
        sig: "rust {\n  pub extern \"C\" fn triple(a: i64) -> i64 { a * 3 }\n}",
        lang: "awk",
        desc: "Inline Rust FFI. A top-level `rust { … }` block is rewritten before lexing into \
               `BEGIN { __rust_compile(\"<base64>\", <line>) }`, which compiles the block and \
               registers its exported functions. Those exports are then callable as barewords \
               from awk. Resolution order at a call site is user awk functions, then language \
               builtins, then the FFI registry — so an export can never shadow either. The `BEGIN` \
               wrapper is required because awk's top level admits only `pattern { action }` rules.",
    },
];

/// `PROCINFO` subscripts populated by `procinfo_refresh` in `src/runtime.rs`
/// and the helpers in `src/procinfo.rs`.
const PROCINFO_KEYS: &[Entry] = &[
    Entry {
        name: r#"PROCINFO["version"]"#,
        sig: r#"PROCINFO["version"]"#,
        lang: "awk",
        desc: "The awkrs package version.",
    },
    Entry {
        name: r#"PROCINFO["api"]"#,
        sig: r#"PROCINFO["api"]"#,
        lang: "awk",
        desc: "Always the string `awkrs`. Where gawk reports its extension-API identity, awkrs \
               names itself — the reliable way for a script to detect that it is running here.",
    },
    Entry {
        name: r#"PROCINFO["api_major"], PROCINFO["api_minor"]"#,
        sig: r#"PROCINFO["api_major"]"#,
        lang: "awk",
        desc: "The extension-API version awkrs reports, 4 and 1.",
    },
    Entry {
        name: r#"PROCINFO["program"]"#,
        sig: r#"PROCINFO["program"]"#,
        lang: "awk",
        desc: "The name the binary was invoked as.",
    },
    Entry {
        name: r#"PROCINFO["platform"]"#,
        sig: r#"PROCINFO["platform"]"#,
        lang: "awk",
        desc: "`posix`, `mingw`, or `unknown` — gawk's vocabulary, deliberately not Rust's \
               `std::env::consts::OS`, so a script testing for `posix` behaves the same here as \
               under gawk.",
    },
    Entry {
        name: r#"PROCINFO["pid"], PROCINFO["ppid"]"#,
        sig: r#"PROCINFO["pid"]"#,
        lang: "awk",
        desc: "Process and parent-process IDs. `ppid` is Unix-only.",
    },
    Entry {
        name: r#"PROCINFO["uid"], PROCINFO["euid"]"#,
        sig: r#"PROCINFO["euid"]"#,
        lang: "awk",
        desc: "Real and effective user IDs (Unix only).",
    },
    Entry {
        name: r#"PROCINFO["gid"], PROCINFO["egid"]"#,
        sig: r#"PROCINFO["gid"]"#,
        lang: "awk",
        desc: "Real and effective group IDs (Unix only).",
    },
    Entry {
        name: r#"PROCINFO["pgrpid"]"#,
        sig: r#"PROCINFO["pgrpid"]"#,
        lang: "awk",
        desc: "Process group ID (Unix only).",
    },
    Entry {
        name: r#"PROCINFO["groupN"]"#,
        sig: r#"PROCINFO["group1"]"#,
        lang: "awk",
        desc: "One key per supplementary group, numbered from `group1` in the order `getgroups` \
               returns them (Unix only).",
    },
    Entry {
        name: r#"PROCINFO["errno"]"#,
        sig: r#"PROCINFO["errno"]"#,
        lang: "awk",
        desc: "The numeric errno behind the most recent failed I/O, the counterpart to `ERRNO`'s \
               message text.",
    },
    Entry {
        name: r#"PROCINFO["FS"]"#,
        sig: r#"PROCINFO["FS"]"#,
        lang: "awk",
        desc: "Which field-splitting rule is currently active: `FS`, `FPAT`, `FIELDWIDTHS`, or \
               `API` in CSV mode. Computed on each refresh from the live variables, not stored.",
    },
    Entry {
        name: r#"PROCINFO["strftime"]"#,
        sig: r#"PROCINFO["strftime"]"#,
        lang: "awk",
        desc: "The format `strftime()` uses when called with no arguments; defaults to \
               `%a %b %e %H:%M:%S %Z %Y`, gawk's `date(1)`-equivalent default.",
    },
    Entry {
        name: r#"PROCINFO["argv"]"#,
        sig: r#"PROCINFO["argv"][0]"#,
        lang: "awk",
        desc: "A nested array of the full process command line, indexed from 0 — including the \
               options that `ARGV` deliberately omits.",
    },
    Entry {
        name: r#"PROCINFO["identifiers"]"#,
        sig: r#"PROCINFO["identifiers"]["split"]"#,
        lang: "awk",
        desc: "A nested array mapping every known name to `builtin`, `scalar`, `array`, or \
               `user`. Built from the compiled program's slot, array, and function tables plus \
               the builtin name list.",
    },
    Entry {
        name: r#"PROCINFO["mb_cur_max"]"#,
        sig: r#"PROCINFO["mb_cur_max"]"#,
        lang: "awk",
        desc: "Maximum bytes per multibyte character in the current locale, best-effort.",
    },
    Entry {
        name: r#"PROCINFO["nproc"]"#,
        sig: r#"PROCINFO["nproc"]"#,
        lang: "awk",
        desc: "Available CPU count. An awkrs addition, useful for choosing a `-j` value from \
               inside a script that re-executes itself.",
    },
    Entry {
        name: r#"PROCINFO["sorted_in"]"#,
        sig: r#"PROCINFO["sorted_in"] = "@ind_num_asc""#,
        lang: "awk",
        desc: "Assignable: sets the traversal order for every subsequent `for (k in array)`. See \
               the Array Traversal Order chapter for the accepted values. Defaults to the empty \
               string, meaning unsorted.",
    },
    Entry {
        name: r#"PROCINFO["prec"], PROCINFO["roundmode"]"#,
        sig: r#"PROCINFO["prec"]"#,
        lang: "awk",
        desc: "Working precision in bits and the MPFR rounding mode. Outside `-M`/`--bignum` the \
               precision reads 53, the width of a double; the rounding mode defaults to `N` \
               (nearest). Both are assignable before the values that depend on them are computed.",
    },
    Entry {
        name: r#"PROCINFO["prec_min"], PROCINFO["prec_max"]"#,
        sig: r#"PROCINFO["prec_min"]"#,
        lang: "awk",
        desc: "MPFR's precision bounds. Present only under `-M`/`--bignum`.",
    },
    Entry {
        name: r#"PROCINFO["gmp_version"], PROCINFO["mpfr_version"]"#,
        sig: r#"PROCINFO["mpfr_version"]"#,
        lang: "awk",
        desc: "Versions of the linked GMP and MPFR libraries, queried from the libraries \
               themselves. Present only under `-M`/`--bignum`.",
    },
    Entry {
        name: r#"PROCINFO["pma"]"#,
        sig: r#"PROCINFO["pma"]"#,
        lang: "awk",
        desc: "gawk's persistent-memory-allocator version. awkrs is not built with PMA, so this \
               key is absent — matching a gawk built without it, rather than reporting a value \
               that would be false.",
    },
    Entry {
        name: r#"PROCINFO["READ_TIMEOUT"]"#,
        sig: r#"PROCINFO["READ_TIMEOUT"] = 500"#,
        lang: "awk",
        desc: "Read timeout in milliseconds. Initialized from the `GAWK_READ_TIMEOUT` environment \
               variable when the script has not set it, and only when that value is positive.",
    },
    Entry {
        name: r#"PROCINFO[input, "READ_TIMEOUT"]"#,
        sig: r#"PROCINFO["-", "READ_TIMEOUT"] = 250"#,
        lang: "awk",
        desc: "Per-input override of the read timeout, keyed by the input name joined with \
               `SUBSEP`. One entry is pre-seeded for every file in `ARGV` plus `-` for standard \
               input, each defaulting to the global timeout.",
    },
    Entry {
        name: r#"PROCINFO[input, "RETRY"]"#,
        sig: r#"PROCINFO["-", "RETRY"] = 1"#,
        lang: "awk",
        desc: "Per-input retry flag, pre-seeded to 0 alongside the per-input read timeouts.",
    },
    Entry {
        name: r#"PROCINFO["awkrs_binmode"]"#,
        sig: r#"PROCINFO["awkrs_binmode"]"#,
        lang: "awk",
        desc: "The current numeric value of `BINMODE`, mirrored here on each refresh. An awkrs \
               key, prefixed so it can never collide with a future gawk one.",
    },
];

/// `PROCINFO["sorted_in"]` values. Source: `parse_sorted_in_at_token` and
/// `sorted_in_mode` in `src/runtime.rs`.
const SORTED_IN: &[Entry] = &[
    Entry {
        name: "@unsorted",
        sig: r#"PROCINFO["sorted_in"] = "@unsorted""#,
        lang: "awk",
        desc: "Hash order — whatever the array's internal iteration produces. The default, and \
               the fastest.",
    },
    Entry {
        name: "@ind_str_asc",
        sig: r#"PROCINFO["sorted_in"] = "@ind_str_asc""#,
        lang: "awk",
        desc: "By index, compared as strings, ascending.",
    },
    Entry {
        name: "@ind_str_desc",
        sig: r#"PROCINFO["sorted_in"] = "@ind_str_desc""#,
        lang: "awk",
        desc: "By index, compared as strings, descending.",
    },
    Entry {
        name: "@ind_num_asc",
        sig: r#"PROCINFO["sorted_in"] = "@ind_num_asc""#,
        lang: "awk",
        desc: "By index, compared as numbers, ascending.",
    },
    Entry {
        name: "@ind_num_desc",
        sig: r#"PROCINFO["sorted_in"] = "@ind_num_desc""#,
        lang: "awk",
        desc: "By index, compared as numbers, descending.",
    },
    Entry {
        name: "@val_str_asc",
        sig: r#"PROCINFO["sorted_in"] = "@val_str_asc""#,
        lang: "awk",
        desc: "By element value, compared as strings, ascending.",
    },
    Entry {
        name: "@val_str_desc",
        sig: r#"PROCINFO["sorted_in"] = "@val_str_desc""#,
        lang: "awk",
        desc: "By element value, compared as strings, descending.",
    },
    Entry {
        name: "@val_num_asc",
        sig: r#"PROCINFO["sorted_in"] = "@val_num_asc""#,
        lang: "awk",
        desc: "By element value, compared as numbers, ascending.",
    },
    Entry {
        name: "@val_num_desc",
        sig: r#"PROCINFO["sorted_in"] = "@val_num_desc""#,
        lang: "awk",
        desc: "By element value, compared as numbers, descending.",
    },
    Entry {
        name: "@val_type_asc",
        sig: r#"PROCINFO["sorted_in"] = "@val_type_asc""#,
        lang: "awk",
        desc: "By value type, ascending: uninitialized, then numbers, then strings, then \
               subarrays.",
    },
    Entry {
        name: "@val_type_desc",
        sig: r#"PROCINFO["sorted_in"] = "@val_type_desc""#,
        lang: "awk",
        desc: "By value type, descending.",
    },
    Entry {
        name: "custom comparison function",
        sig: r#"PROCINFO["sorted_in"] = "my_cmp""#,
        lang: "awk",
        desc: "A bare identifier names a user function used as the comparator. It must take 2 \
               parameters (the two indices) or 4 (index, value, index, value) and return a \
               negative number, zero, or a positive number. A wrong arity is a runtime error \
               naming the function. An unrecognized `@…` token falls back to unsorted with a \
               one-time warning on standard error.",
    },
    Entry {
        name: "under --posix",
        sig: "awkrs --posix -f prog.awk",
        lang: "sh",
        desc:
            "`-P`/`--posix` forces unsorted traversal regardless of what `PROCINFO[\"sorted_in\"]` \
               says — POSIX awk specifies no ordering, so the setting is ignored rather than \
               rejected.",
    },
];

/// Command-line options. Source: the `Args` derive in `src/cli.rs` and the
/// dispatch in `src/lib.rs` / `src/cli_effects.rs`.
const CLI_OPTIONS: &[Entry] = &[
    Entry {
        name: "-f, --file",
        sig: "awkrs -f prog.awk [-f more.awk] input",
        lang: "sh",
        desc: "Read the program from a file. Repeatable; the sources are concatenated in order. \
               A single `-f` with no other source-shaping flag is also the only form eligible for \
               the compiled-bytecode cache.",
    },
    Entry {
        name: "-F, --field-separator",
        sig: "awkrs -F: '{ print $1 }' /etc/passwd",
        lang: "sh",
        desc: "Set `FS` before the program runs. Accepts the attached form `-F:` as well as the \
               separated one.",
    },
    Entry {
        name: "-v, --assign",
        sig: "awkrs -v n=3 '{ print $n }' input",
        lang: "sh",
        desc: "Assign a global before `BEGIN` runs, with escape sequences processed. Repeatable, \
               and the attached form `-vn=3` works.",
    },
    Entry {
        name: "-e, --source",
        sig: "awkrs -e 'BEGIN { print 1 }' -e 'END { print 2 }'",
        lang: "sh",
        desc: "Add program text on the command line. Repeatable, and mixable with `-f`.",
    },
    Entry {
        name: "-i, --include",
        sig: "awkrs -i lib.awk -e 'BEGIN { helper() }'",
        lang: "sh",
        desc: "Include a library file, the command-line equivalent of `@include`. Repeatable.",
    },
    Entry {
        name: "-l, --load",
        sig: "awkrs -l mylib -e 'BEGIN { f() }'",
        lang: "sh",
        desc: "Load an extension by name, resolved against `AWKPATH` (default `.`), trying \
               `NAME.awk` then `NAME` in each directory. Not found is an error naming both \
               candidates. Repeatable.",
    },
    Entry {
        name: "-E, --exec",
        sig: "#!/usr/bin/env -S awkrs -E",
        lang: "sh",
        desc: "Read the program from a file and treat every remaining argument as data, never as \
               an option. The safe form for `#!` scripts, since it stops a data filename that \
               starts with `-` from being parsed as a flag.",
    },
    Entry {
        name: "-b, --characters-as-bytes",
        sig: "awkrs -b '{ print length($0) }' input",
        lang: "sh",
        desc: "Treat input bytes as characters. `length`, `substr`, and `index` then count and \
               slice bytes rather than Unicode scalars.",
    },
    Entry {
        name: "-c, --traditional",
        sig: "awkrs -c 'BEGIN { print length(\"x\") }'",
        lang: "sh",
        desc: "Traditional awk compatibility: the gawk-extension builtins are refused, and the \
               BSD awk zero-padding quirk for `%0Ns` / `%0Nc` is enabled.",
    },
    Entry {
        name: "-P, --posix",
        sig: "awkrs -P -f prog.awk input",
        lang: "sh",
        desc: "Strict POSIX mode: refuses the same extension builtins as `--traditional` and \
               forces unsorted array traversal.",
    },
    Entry {
        name: "-n, --non-decimal-data",
        sig: "awkrs -n '{ print $1 + 0 }' hex.txt",
        lang: "sh",
        desc: "Recognize `0x…` and leading-zero octal numbers in input data, not just in program \
               text.",
    },
    Entry {
        name: "-M, --bignum",
        sig: "awkrs -M 'BEGIN { print 2 ^ 200 }'",
        lang: "sh",
        desc: "Arbitrary-precision arithmetic through GMP/MPFR. Integer literals written without \
               a decimal point keep their exact digits instead of rounding through a double, and \
               `PROCINFO[\"prec\"]` / `PROCINFO[\"roundmode\"]` control the working precision.",
    },
    Entry {
        name: "-N, --use-lc-numeric",
        sig: "awkrs -N 'BEGIN { printf \"%\\x27d\\n\", 1234567 }'",
        lang: "sh",
        desc: "Honor `LC_NUMERIC` in `printf`/`sprintf`/`print` output and in `CONVFMT`/`OFMT` \
               formatting, including the `%'` grouping flag. Input coercion is deliberately \
               unaffected — `$1 + 0` still reads `.` as the radix point.",
    },
    Entry {
        name: "-k, --csv",
        sig: "awkrs --csv '{ print $2 }' data.csv",
        lang: "sh",
        desc: "CSV mode: comma-separated with quoted fields and `\"\"` as the embedded-quote \
               escape. Reported as `PROCINFO[\"FS\"] == \"API\"`.",
    },
    Entry {
        name: "-r, --re-interval",
        sig: "awkrs -r '/a{2,3}/' input",
        lang: "sh",
        desc: "Accepted and ignored. `{m,n}` interval expressions are always available, so the \
               flag exists only so that old command lines keep working.",
    },
    Entry {
        name: "-O, --optimize",
        sig: "awkrs -O -f prog.awk input",
        lang: "sh",
        desc: "Accepted for gawk compatibility. Optimization and the JIT are on by default, so \
               this changes nothing; `-s` is what turns them off.",
    },
    Entry {
        name: "-s, --no-optimize",
        sig: "awkrs -s -f prog.awk input",
        lang: "sh",
        desc: "Disable optimization and the JIT, forcing the plain interpreter. The first thing \
               to try when a program's results differ between runs — if `-s` changes the answer, \
               the bug is in a compiled path.",
    },
    Entry {
        name: "-j, --threads",
        sig: "awkrs -j 8 '{ print $1 }' big.log",
        lang: "sh",
        desc: "Worker threads for the parallel record engine (default 1). Only programs the \
               parallel-safety analyzer accepts run in parallel; anything with cross-record state \
               falls back to sequential execution.",
    },
    Entry {
        name: "--read-ahead",
        sig: "awkrs -j 4 --read-ahead 4096 '{ … }'",
        lang: "sh",
        desc: "Lines per batch when reading standard input in parallel mode (default 1024). Each \
               batch is processed in parallel and printed in order before the next is read, so \
               output ordering is preserved.",
    },
    Entry {
        name: "-S, --sandbox",
        sig: "awkrs -S -f untrusted.awk input",
        lang: "sh",
        desc: "Disable `system()`, the file-I/O extension functions, pipes, coprocesses, and the \
               network pseudo-paths. Each blocked call fails with a `sandbox:` runtime error \
               rather than silently doing nothing.",
    },
    Entry {
        name: "-L, --lint",
        sig: "awkrs -L fatal -f prog.awk input",
        lang: "sh",
        desc: "Enable lint warnings. The optional value selects `fatal`, `invalid`, or `no-ext` \
               behavior.",
    },
    Entry {
        name: "-t, --lint-old",
        sig: "awkrs -t -f prog.awk input",
        lang: "sh",
        desc: "Warn about constructs that will not port to old awk implementations.",
    },
    Entry {
        name: "-d, --dump-variables",
        sig: "awkrs -d vars.out -f prog.awk input",
        lang: "sh",
        desc: "After the run, dump the final global variable state. Takes an optional path; with \
               no value, or `-`, the dump goes to standard output.",
    },
    Entry {
        name: "-D, --debug",
        sig: "awkrs -D -f prog.awk input",
        lang: "sh",
        desc: "List the program's rules and functions for debugging. Optional path; defaults to \
               standard error.",
    },
    Entry {
        name: "-o, --pretty-print",
        sig: "awkrs -o formatted.awk -f prog.awk",
        lang: "sh",
        desc:
            "Emit a re-indented program listing rebuilt from the AST. Optional path; defaults to \
               standard output. The layout is awkrs's own — it is not byte-compatible with gawk's \
               `--pretty-print`.",
    },
    Entry {
        name: "-p, --profile",
        sig: "awkrs -p prof.txt -f prog.awk input",
        lang: "sh",
        desc: "Wall-clock summary with per-rule hit counts. Optional path; defaults to standard \
               output. An awkrs-format report, not gawk's `awkprof.out`.",
    },
    Entry {
        name: "-g, --gen-pot",
        sig: "awkrs --gen-pot -f prog.awk",
        lang: "sh",
        desc: "Scan the program for translatable strings and emit a `.pot` template.",
    },
    Entry {
        name: "-I, --trace",
        sig: "awkrs -I -f prog.awk input",
        lang: "sh",
        desc: "Trace opcode execution as the program runs.",
    },
    Entry {
        name: "-C, --copyright",
        sig: "awkrs --copyright",
        lang: "sh",
        desc: "Print the copyright notice and exit.",
    },
    Entry {
        name: "-W",
        sig: "awkrs -W version",
        lang: "sh",
        desc: "mawk/BusyBox-style option bundle, comma-separated. `help`/`usage`, `version`/`v`, \
               and `dump` act and exit; `exec=FILE` behaves like `-E`; `sprintf=N`, `posix_space`, \
               `interactive`, and `random` are accepted and ignored so mawk command lines keep \
               working.",
    },
    Entry {
        name: "--repl",
        sig: "awkrs --repl",
        lang: "sh",
        desc: "Launch the interactive REPL. Also the default when awkrs is started on a terminal \
               with no program and no input files.",
    },
    Entry {
        name: "--lsp",
        sig: "awkrs --lsp",
        lang: "sh",
        desc: "Run as a Language Server over stdio: diagnostics, completion, hover, document \
               symbols, signature help, goto-definition, references, highlights, and folding \
               ranges. Nothing is written to the terminal — the process speaks JSON-RPC only.",
    },
    Entry {
        name: "--dap",
        sig: "awkrs --dap 127.0.0.1:4711 -f prog.awk input",
        lang: "sh",
        desc:
            "Run as a Debug Adapter. With no value it speaks DAP over stdio; with `HOST:PORT` it \
               connects to that address instead, which leaves the debugged program's own standard \
               output free — the mode IDE plugins use.",
    },
    Entry {
        name: "--dump-tokens",
        sig: "awkrs --dump-tokens 'BEGIN { x = 1 + 2 }'",
        lang: "sh",
        desc: "Print the lexer token stream, one `line<TAB>token` per line, and exit. Runs after \
               `rust { }` desugaring, so an FFI block shows up as the `__rust_compile` call it \
               becomes.",
    },
    Entry {
        name: "--dump-ast",
        sig: "awkrs --dump-ast 'BEGIN { x = 1 + 2 }'",
        lang: "sh",
        desc: "Print the parsed AST and exit.",
    },
    Entry {
        name: "--dump-bytecode",
        sig: "awkrs --dump-bytecode 'BEGIN { x = 1 + 2 }'",
        lang: "sh",
        desc: "Print the compiled bytecode ops, chunk by chunk, and exit.",
    },
    Entry {
        name: "--disasm",
        sig: "awkrs --disasm 'BEGIN { x = 1 + 2 }'",
        lang: "sh",
        desc: "Print a fusevm disassembly listing — index, line, and mnemonic per op, with the \
               chunk's name table — and exit. The readable counterpart to `--dump-bytecode`.",
    },
    Entry {
        name: "--tiers",
        sig: "awkrs --tiers -f prog.awk input",
        lang: "sh",
        desc:
            "Run the program on the fusevm backend and then report which execution tier actually \
               took each chunk: op count, whether the chunk was block-JIT eligible and compiled, \
               the largest JIT-eligible region, and per-loop trace status. The answers come from \
               fusevm's own predicates after the run, so this distinguishes *the JIT was enabled* \
               from *the JIT compiled this*. A program outside the backend's coverage says so \
               instead of reporting a tier.",
    },
    Entry {
        name: "--aot",
        sig: "awkrs --aot ./prog 'BEGIN { print 42 }'",
        lang: "sh",
        desc: "Ahead-of-time compile a `BEGIN`-only program to a native executable at the given \
               path, via a Cranelift object linked against the awk runtime.",
    },
    Entry {
        name: "-h, --help",
        sig: "awkrs --help",
        lang: "sh",
        desc: "Print the help screen and exit.",
    },
    Entry {
        name: "-V, --version",
        sig: "awkrs --version",
        lang: "sh",
        desc: "Print the version and exit.",
    },
    Entry {
        name: "--",
        sig: "awkrs -- '{ print }' -weird-filename",
        lang: "sh",
        desc:
            "End option parsing. Everything after it is program text and input files, even if it \
               begins with a dash.",
    },
];

/// Environment variables awkrs reads. Each entry names the module that reads it.
const ENV_VARS: &[Entry] = &[
    Entry {
        name: "AWKPATH",
        sig: "AWKPATH=/usr/local/share/awk:. awkrs -l mylib …",
        lang: "sh",
        desc:
            "Colon-separated search path for `-l`/`--load`, defaulting to `.`. Each directory is \
               tried with `NAME.awk` and then bare `NAME`.",
    },
    Entry {
        name: "AWKRS_CACHE",
        sig: "AWKRS_CACHE=0 awkrs -f prog.awk input",
        lang: "sh",
        desc: "Set to `0`, `false`, or `no` to disable the compiled-bytecode cache in \
               `~/.awkrs/scripts.rkyv`. Any other value, or no value, leaves it enabled. The cache \
               is keyed on the script's modification time and the awkrs binary's, so a rebuilt \
               interpreter invalidates it automatically.",
    },
    Entry {
        name: "AWKRS_JIT",
        sig: "AWKRS_JIT=0 awkrs -f prog.awk input",
        lang: "sh",
        desc: "Set to `0` to disable the JIT, the environment equivalent of `-s`/`--no-optimize`. \
               `-s` wins regardless of this variable.",
    },
    Entry {
        name: "AWKRS_FUSEVM",
        sig: "AWKRS_FUSEVM=0 awkrs -f prog.awk input",
        lang: "sh",
        desc: "Set to `0` to stop the interpreter offloading eligible numeric chunks to fusevm, \
               forcing awkrs's own opcode loop. On by default, and read once per process — \
               changing it mid-run has no effect.",
    },
    Entry {
        name: "AWKRS_FUSEVM_NATIVE",
        sig: "AWKRS_FUSEVM_NATIVE=1 awkrs -f prog.awk input",
        lang: "sh",
        desc: "Set to `1` to compile and run the whole program on the fusevm backend rather than \
               the `vm.rs` interpreter. The validation harness for the ongoing migration: coverage \
               is partial, and a construct the backend does not support is an error rather than a \
               silent fallback.",
    },
    Entry {
        name: "AWKRS_AOT_RUNTIME_LIB",
        sig: "AWKRS_AOT_RUNTIME_LIB=/path/libawkrs_rt.a awkrs --aot ./prog …",
        lang: "sh",
        desc: "Override the runtime library the `--aot` linker step links against.",
    },
    Entry {
        name: "AWKRS_REPL_MODE",
        sig: "AWKRS_REPL_MODE=vi awkrs --repl",
        lang: "sh",
        desc:
            "REPL edit mode, `emacs` or `vi` (`vim` is accepted for `vi`). Overrides the `[repl] \
               mode` setting in `~/.awkrs/config.toml`; the default is emacs.",
    },
    Entry {
        name: "AWKRS_NO_CONFIG",
        sig: "AWKRS_NO_CONFIG=1 awkrs --repl",
        lang: "sh",
        desc:
            "Set to any value to stop the REPL seeding a default `~/.awkrs/config.toml` on first \
               launch. For CI and sandboxes that must not write to the home directory.",
    },
    Entry {
        name: "AWKRS_DAP_LOG",
        sig: "AWKRS_DAP_LOG=1 awkrs --dap -f prog.awk input",
        lang: "sh",
        desc: "Set to any value to enable debug-adapter protocol logging.",
    },
    Entry {
        name: "GAWK_READ_TIMEOUT",
        sig: "GAWK_READ_TIMEOUT=500 awkrs -f prog.awk",
        lang: "sh",
        desc: "Default read timeout in milliseconds, used to seed `PROCINFO[\"READ_TIMEOUT\"]` \
               when the script has not set it. Values are clamped to a non-negative 32-bit range, \
               and a non-positive value leaves the key unset.",
    },
    Entry {
        name: "LC_NUMERIC",
        sig: "LC_NUMERIC=en_US.UTF-8 awkrs -N 'BEGIN { … }'",
        lang: "sh",
        desc: "Under `-N`/`--use-lc-numeric`, supplies the decimal point and thousands separator \
               for output formatting. A locale with no thousands separator makes the `%'` flag a \
               no-op.",
    },
    Entry {
        name: "LANGUAGE, LC_ALL, LC_MESSAGES, LANG",
        sig: "LANG=fr_FR.UTF-8 awkrs -f prog.awk",
        lang: "sh",
        desc: "Consulted in that order to pick the gettext message catalog for `dcgettext` and \
               `dcngettext`; the first one set wins.",
    },
    Entry {
        name: "NO_COLOR, CLICOLOR_FORCE",
        sig: "NO_COLOR=1 awkrs --help",
        lang: "sh",
        desc: "Control the colorized help output. `NO_COLOR` set to anything disables color \
               entirely; otherwise color is used when standard output is a terminal, or when \
               `CLICOLOR_FORCE` is set.",
    },
];

fn build_page() -> String {
    // Collect (chapter, [(name, markdown)]) in declaration order, warning on
    // any name the corpus doesn't document so nothing silently vanishes.
    let mut chapters: Vec<(&'static str, Vec<(&'static str, String)>)> = Vec::new();
    for ch in CHAPTERS {
        let mut rows: Vec<(&'static str, String)> = Vec::new();
        match &ch.entries {
            Entries::Corpus(names) => {
                for &name in *names {
                    match doc_markdown(name) {
                        Some(md) => rows.push((name, md)),
                        None => eprintln!("warning: no doc for `{name}` (chapter `{}`)", ch.title),
                    }
                }
            }
            Entries::Local(entries) => {
                for e in *entries {
                    rows.push((
                        e.name,
                        format!("```{}\n{}\n```\n\n{}", e.lang, e.sig, e.desc),
                    ));
                }
            }
        }
        if !rows.is_empty() {
            chapters.push((ch.title, rows));
        }
    }

    let total_topics: usize = chapters.iter().map(|(_, r)| r.len()).sum();
    let chapter_count = chapters.len();

    // ── render ──────────────────────────────────────────────────────────
    let mut out = String::with_capacity(256 * 1024);
    out.push_str(HEAD);
    out.push_str(&format!(
        r#"  <header class="tutorial-header">
    <div class="tutorial-header-inner">
      <div>
        <h1 class="tutorial-brand">// AWKRS — FULL REFERENCE</h1>
        <nav class="tutorial-crumbs" aria-label="Breadcrumb">
          <a href="index.html">Docs</a>
          <span class="sep">/</span>
          <span class="current">Reference</span>
          <span class="sep">/</span>
          <a href="https://github.com/MenkeTechnologies/awkrs" target="_blank" rel="noopener noreferrer">GitHub</a>
        </nav>
        <p class="docs-build-line">awkrs v{version} · {total_topics} topics · {chapter_count} chapters · generated from <code>awkrs/src/lsp.rs</code> and <code>awkrs/src/bin/gen_docs.rs</code></p>
      </div>
      <div class="tutorial-toolbar">
        <button type="button" class="btn btn-secondary" id="btnTheme" title="Toggle light/dark">Theme</button>
        <button type="button" class="btn btn-secondary active" id="btnCrt" title="CRT scanline overlay">CRT</button>
        <button type="button" class="btn btn-secondary active" id="btnNeon" title="Neon border pulse">Neon</button>
        <a class="btn btn-secondary" href="index.html">Hub</a>
        <a class="btn btn-secondary" href="https://github.com/MenkeTechnologies/awkrs" target="_blank" rel="noopener noreferrer">GitHub</a>
      </div>
    </div>
  </header>

  <div class="hub-scheme-strip">
    <div class="hub-scheme-strip-inner">
      <span class="hud-scheme-label">// Color scheme</span>
      <div class="scheme-grid" id="hudSchemeGrid"></div>
    </div>
  </div>

  <main class="tutorial-main">
    <h2 class="tutorial-title"><span class="step-hash">&gt;_</span>LANGUAGE REFERENCE</h2>
    <p class="tutorial-subtitle">Every builtin, special variable, keyword, operator, redirection, format specifier, directive, command-line option, and environment variable awkrs implements — each with its signature and a description written from the implementation. Identifier entries render from the exact markdown the awkrs LSP shows on hover. Jump via the chapter index, or <kbd>Ctrl+F</kbd> for a specific name.</p>
"#,
        version = env!("CARGO_PKG_VERSION"),
        total_topics = total_topics,
        chapter_count = chapter_count,
    ));

    // Chapter index
    out.push_str(
        r#"    <section class="tutorial-section">
      <h2>Chapters</h2>
      <ul class="chapter-index">
"#,
    );
    for (chapter, rows) in &chapters {
        let slug = slugify(chapter);
        out.push_str(&format!(
            "        <li><a href=\"#ch-{slug}\">{chapter}</a> <span class=\"chapter-count\">{n}</span></li>\n",
            slug = slug,
            chapter = html_escape(chapter),
            n = rows.len(),
        ));
    }
    out.push_str("      </ul>\n    </section>\n");

    // Chapters and their topics
    let mut seen_anchors: HashSet<String> = HashSet::new();
    for (chapter, rows) in &chapters {
        let slug = slugify(chapter);
        out.push_str(&format!(
            r#"    <section class="tutorial-section" id="ch-{slug}">
      <h2>{chapter}</h2>
      <p class="chapter-meta">{n} topics</p>
"#,
            slug = slug,
            chapter = html_escape(chapter),
            n = rows.len(),
        ));
        for (topic, md) in rows {
            let topic_slug = unique_anchor(topic, &mut seen_anchors);
            let topic_escaped = html_escape(topic);
            out.push_str("      <article class=\"doc-entry\" id=\"doc-");
            out.push_str(&topic_slug);
            out.push_str("\">\n        <h3><a class=\"doc-anchor\" href=\"#doc-");
            out.push_str(&topic_slug);
            out.push_str("\">#</a> <code>");
            out.push_str(&topic_escaped);
            out.push_str("</code></h3>\n");
            out.push_str(&markdown_to_html(md));
            out.push_str("      </article>\n");
        }
        out.push_str("    </section>\n");
    }

    out.push_str(FOOT);
    out
}

// ─────────────────────────────────────────────────────────────────────────
// Minimal markdown → HTML converter. Scope: what the LSP corpus actually
// uses. Blocks: fenced code, `### heading`, blank-line-separated paragraphs,
// `-`/`*` bullet lists. Inlines: `backtick code` and `**bold**`. Everything
// else is HTML-escaped and passes through as plain text.
// ─────────────────────────────────────────────────────────────────────────
fn markdown_to_html(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + md.len() / 4);
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Fenced code block: ```LANG … ```
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            let lang = rest.trim().to_string();
            let lang_attr = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"lang-{}\"", html_escape(&lang))
            };
            out.push_str(&format!("        <pre><code{lang_attr}>"));
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                out.push_str(&html_escape(l));
                out.push('\n');
                i += 1;
            }
            out.push_str("</code></pre>\n");
            continue;
        }

        // Heading: `###` (the only level the corpus uses).
        if let Some(body) = line.strip_prefix("### ") {
            out.push_str(&format!("        <h4>{}</h4>\n", inline(body)));
            i += 1;
            continue;
        }
        if let Some(body) = line.strip_prefix("## ") {
            out.push_str(&format!("        <h4>{}</h4>\n", inline(body)));
            i += 1;
            continue;
        }

        // Bullet list.
        if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
            out.push_str("        <ul>\n");
            while i < lines.len() {
                let l = lines[i];
                let t = l.trim_start();
                let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
                    break;
                };
                out.push_str(&format!("          <li>{}</li>\n", inline(item)));
                i += 1;
            }
            out.push_str("        </ul>\n");
            continue;
        }

        // Blank line → paragraph boundary.
        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        // Paragraph: accumulate contiguous non-blank, non-block lines.
        let mut para = String::new();
        while i < lines.len() {
            let l = lines[i];
            let t = l.trim_start();
            if l.trim().is_empty()
                || t.starts_with("```")
                || t.starts_with("### ")
                || t.starts_with("## ")
                || t.starts_with("- ")
                || t.starts_with("* ")
            {
                break;
            }
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(l.trim());
            i += 1;
        }
        if !para.is_empty() {
            out.push_str(&format!("        <p>{}</p>\n", inline(&para)));
        }
    }
    out
}

/// Inline pass: `backtick code` spans and `**bold**` spans, otherwise
/// HTML-escape. Single `*em*` is intentionally not supported because awk
/// docs contain `*` in expressions; bold uses doubled `**`.
fn inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if bytes[i] == b'`' {
            // Find matching backtick.
            let start = i + 1;
            let mut j = start;
            while j < s.len() && bytes[j] != b'`' {
                j += 1;
            }
            if j < s.len() {
                out.push_str("<code>");
                out.push_str(&html_escape(&s[start..j]));
                out.push_str("</code>");
                i = j + 1;
                continue;
            }
        }
        // `**bold**`. Require the closing `**` to also exist; otherwise fall
        // through and treat the literal `**` as text.
        if i + 1 < s.len() && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < s.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    break;
                }
                j += 1;
            }
            if j + 1 < s.len() && bytes[j] == b'*' && bytes[j + 1] == b'*' {
                out.push_str("<strong>");
                out.push_str(&inline(&s[start..j]));
                out.push_str("</strong>");
                i = j + 2;
                continue;
            }
        }
        // Default: html-escape this one char.
        let c = &s[i..i + char_len(bytes, i)];
        out.push_str(&html_escape(c));
        i += c.len();
    }
    out
}

fn char_len(bytes: &[u8], i: usize) -> usize {
    let b = bytes[i];
    if b < 0x80 {
        1
    } else if b & 0xE0 == 0xC0 {
        2
    } else if b & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Anchor for one entry heading, guaranteed unique across the whole page.
///
/// [`slugify`] drops every non-alphanumeric character, so symbol-only headings
/// (`++`, `$`, `%%`, `\n`) would all collapse to the same empty anchor and the
/// `#` links would land on whichever entry rendered first. Fall back to a hex
/// encoding of the bytes for those, then disambiguate anything still colliding
/// with a numeric suffix.
fn unique_anchor(name: &str, seen: &mut HashSet<String>) -> String {
    let mut base = slugify(name);
    if base.is_empty() {
        base = "sym".to_string();
        for b in name.bytes() {
            base.push_str(&format!("-{b:02x}"));
        }
    }
    let mut slug = base.clone();
    let mut n = 2;
    while !seen.insert(slug.clone()) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    slug
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

const HEAD: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="dark light">
  <meta name="description" content="awkrs full reference — every builtin, special variable, keyword, operator, format specifier, directive, command-line option, and environment variable, each with a signature and a description written from the implementation.">
  <title>awkrs — Reference</title>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Orbitron:wght@400;600;700;900&amp;family=Share+Tech+Mono&amp;display=swap" rel="stylesheet">
  <link rel="stylesheet" href="hud-static.css">
  <link rel="stylesheet" href="tutorial.css">
  <style>
    .tutorial-main { max-width: 72rem; }
    .docs-build-line {
      margin: 0.35rem 0 0;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 11px; color: var(--text-dim);
      letter-spacing: 0.03em; max-width: 42rem; opacity: 0.75;
    }
    .hub-scheme-strip {
      border-bottom: 1px dashed var(--border);
      background: color-mix(in srgb, var(--bg-secondary) 85%, transparent);
      padding: 0.55rem 1.5rem 0.65rem; position: relative;
    }
    .hub-scheme-strip-inner {
      max-width: 72rem; margin: 0 auto;
      display: flex; align-items: center; gap: 0.85rem;
    }
    .hub-scheme-strip .hud-scheme-label {
      flex: 0 0 auto;
      font-family: 'Orbitron', sans-serif; font-size: 9px; font-weight: 700;
      letter-spacing: 2px; text-transform: uppercase; color: var(--accent);
    }
    .hub-scheme-strip .scheme-grid {
      flex: 1 1 auto;
      display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 6px;
    }
    @media (max-width: 720px) {
      .hub-scheme-strip-inner { flex-direction: column; align-items: stretch; }
      .hub-scheme-strip .scheme-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    }

    .chapter-index {
      list-style: none; padding: 0; margin: 0;
      display: grid; grid-template-columns: repeat(auto-fill, minmax(18rem, 1fr));
      gap: 0.3rem;
    }
    .chapter-index li {
      border: 1px solid var(--border); padding: 0.45rem 0.65rem; border-radius: 2px;
      background: color-mix(in srgb, var(--bg-card) 92%, transparent);
      display: flex; justify-content: space-between; align-items: baseline;
    }
    .chapter-index li a {
      color: var(--cyan); text-decoration: none; font-size: 13px;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }
    .chapter-index li a:hover { color: var(--accent-light); }
    .chapter-count {
      font-size: 10px; color: var(--text-muted);
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }
    .chapter-meta {
      font-size: 11px; color: var(--text-muted); margin: -0.3rem 0 0.8rem;
      font-family: 'Share Tech Mono', ui-monospace, monospace;
    }

    .doc-entry {
      margin: 1rem 0 1.4rem;
      padding: 0.75rem 0.9rem 0.5rem;
      border-left: 2px solid var(--cyan);
      background: color-mix(in srgb, var(--bg) 94%, transparent);
      border-radius: 2px;
    }
    .doc-entry h3 {
      margin: 0 0 0.45rem;
      font-family: 'Orbitron', sans-serif;
      font-size: 13px; font-weight: 700; letter-spacing: 1.5px;
      text-transform: uppercase; color: var(--cyan);
    }
    .doc-entry h3 code {
      color: var(--accent-light); background: transparent; border: none;
      padding: 0; font-size: 1em; letter-spacing: 0.5px;
    }
    .doc-entry .doc-anchor {
      color: var(--text-muted); font-size: 0.85em; margin-right: 0.25rem;
      text-decoration: none;
    }
    .doc-entry .doc-anchor:hover { color: var(--accent); }
    .doc-entry h4 {
      font-family: 'Orbitron', sans-serif;
      font-size: 11px; font-weight: 700; letter-spacing: 1.5px;
      text-transform: uppercase; color: var(--accent-light);
      margin: 0.8rem 0 0.3rem;
    }
    .doc-entry p {
      font-size: 13px; line-height: 1.6; color: var(--text-dim);
      margin: 0.35rem 0;
    }
    .doc-entry p code, .doc-entry li code {
      color: var(--accent-light); font-size: 12px;
    }
    .doc-entry ul { margin: 0.3rem 0 0.5rem; padding-left: 1.25rem; }
    .doc-entry li { font-size: 13px; color: var(--text-dim); line-height: 1.55; margin: 0.2rem 0; }
    .doc-entry pre {
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 12px;
      background: var(--bg); border: 1px solid var(--border);
      border-radius: 2px;
      padding: 0.7rem 0.9rem; overflow-x: auto;
      color: var(--text); margin: 0.5rem 0;
      box-shadow: inset 0 0 18px rgba(0, 0, 0, 0.35);
    }
    .doc-entry pre code { color: var(--text); background: transparent; border: none; padding: 0; }
    [data-theme="light"] .doc-entry pre { box-shadow: inset 0 0 10px rgba(0, 0, 0, 0.05); }

    kbd {
      font-family: 'Share Tech Mono', ui-monospace, monospace;
      font-size: 11px;
      padding: 1px 6px;
      background: var(--bg-secondary);
      border: 1px solid var(--border);
      border-bottom-width: 2px;
      border-radius: 3px;
      color: var(--cyan);
    }
  </style>
</head>
<body>
  <div class="app tutorial-app" id="docsApp">
    <div class="crt-scanline" id="crtH" aria-hidden="true"></div>
    <div class="crt-scanline-v" id="crtV" aria-hidden="true"></div>
"##;

const FOOT: &str = r#"  </main>
  </div>
  <script src="hud-theme.js"></script>
</body>
</html>
"#;
