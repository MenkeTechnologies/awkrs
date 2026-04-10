//! Abstract syntax tree for awk programs (rules + optional user functions).

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub rules: Vec<Rule>,
    pub funcs: HashMap<String, FunctionDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub pattern: Pattern,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Begin,
    End,
    /// gawk-style: run before each input file (after `BEGIN`).
    BeginFile,
    /// gawk-style: run after each input file (before `END`).
    EndFile,
    Expr(Expr),
    Regexp(String),
    /// Inclusive range: two patterns (`/a/,/b/` or `NR==1,NR==5`).
    Range(Box<Pattern>, Box<Pattern>),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    If {
        cond: Expr,
        then_: Vec<Stmt>,
        else_: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    /// `do { … } while (cond)` — body runs at least once; `continue` jumps to the condition test.
    DoWhile {
        body: Vec<Stmt>,
        cond: Expr,
    },
    ForC {
        init: Option<Expr>,
        cond: Option<Expr>,
        iter: Option<Expr>,
        body: Vec<Stmt>,
    },
    ForIn {
        var: String,
        arr: String,
        body: Vec<Stmt>,
    },
    Block(Vec<Stmt>),
    Expr(Expr),
    /// `print` / `print expr-list` with optional `> file` or `>> file`.
    Print {
        args: Vec<Expr>,
        redir: Option<PrintRedir>,
    },
    /// `printf fmt, expr-list` (statement form, like `print`) with the same redirections.
    Printf {
        args: Vec<Expr>,
        redir: Option<PrintRedir>,
    },
    Break,
    Continue,
    Next,
    /// Skip remaining records in the current input file (POSIX / gawk).
    NextFile,
    Exit(Option<Expr>),
    Delete {
        name: String,
        /// `None` = delete entire array; `Some(vec)` = delete one key (possibly multidimensional).
        indices: Option<Vec<Expr>>,
    },
    Return(Option<Expr>),
    /// `getline` / `getline var` / `getline < file` / `expr | getline [var]` / …
    GetLine {
        /// `expr | getline` — shell command string from `expr` (via `sh -c`).
        pipe_cmd: Option<Box<Expr>>,
        var: Option<String>,
        redir: GetlineRedir,
    },
    /// gawk-style `switch (expr) { case … default … }` (cases do not fall through).
    Switch {
        expr: Expr,
        arms: Vec<SwitchArm>,
    },
}

/// One arm of a `switch` statement.
#[derive(Debug, Clone, PartialEq)]
pub enum SwitchArm {
    Case {
        label: SwitchLabel,
        stmts: Vec<Stmt>,
    },
    Default {
        stmts: Vec<Stmt>,
    },
}

/// `case` label: expression equality or regex match (`case /re/`).
#[derive(Debug, Clone, PartialEq)]
pub enum SwitchLabel {
    Expr(Expr),
    Regexp(String),
}

/// Output redirection on `print` / `printf` statements.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintRedir {
    /// Truncate on first open (same as POSIX `>`).
    Overwrite(Box<Expr>),
    /// Append on first open (`>>`).
    Append(Box<Expr>),
    /// One-way pipe: `| expr` runs `sh -c` with that string; writes go to the subprocess stdin.
    Pipe(Box<Expr>),
    /// Two-way pipe: `|& expr` — same shell command model; stdin and stdout are both connected.
    Coproc(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GetlineRedir {
    /// Same stream as main input (or stdin).
    Primary,
    /// `getline ... < expr`
    File(Box<Expr>),
    /// `getline ... <& expr` — read from the stdout of the coprocess (same command string as `|&`).
    Coproc(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    /// Decimal integer from source with no `.` — preserved as digits for **`-M`** (see [`crate::bytecode::Op::PushNumDecimalStr`]).
    IntegerLiteral(String),
    Str(String),
    /// gawk-style regexp constant: `@/pattern/` — value type is **regexp**, not string (`typeof` is **`"regexp"`**).
    RegexpLiteral(String),
    Var(String),
    Field(Box<Expr>),
    Index {
        name: String,
        /// One or more indices; multiple are joined with `SUBSEP` (multidimensional arrays).
        indices: Vec<Expr>,
    },
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Assign {
        name: String,
        op: Option<BinOp>,
        rhs: Box<Expr>,
    },
    AssignField {
        field: Box<Expr>,
        op: Option<BinOp>,
        rhs: Box<Expr>,
    },
    AssignIndex {
        name: String,
        indices: Vec<Expr>,
        op: Option<BinOp>,
        rhs: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// Indirect call: `@expr(args)` — `expr` must yield the function name (gawk).
    IndirectCall {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        then_: Box<Expr>,
        else_: Box<Expr>,
    },
    /// `key in array` — membership test (array is a name, not an expression).
    In {
        key: Box<Expr>,
        arr: String,
    },
    /// Parenthesized comma list `(e1, e2, …)` — gawk: multidimensional `in` key and lone `print` arg.
    Tuple(Vec<Expr>),
    /// `++` / `--` on a scalar, field, or array element (gawk-style).
    IncDec {
        op: IncDecOp,
        target: IncDecTarget,
    },
    /// `getline` as an expression — yields `1` (record), `0` (EOF), or `-1` (error).
    /// Same shape as [`Stmt::GetLine`]; used in `if ((getline x) > 0)` and `expr | getline`.
    GetLine {
        pipe_cmd: Option<Box<Expr>>,
        var: Option<String>,
        redir: GetlineRedir,
    },
}

/// Prefix or postfix `++` / `--`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    PreInc,
    PostInc,
    PreDec,
    PostDec,
}

/// Lvalue for `++` / `--` only.
#[derive(Debug, Clone, PartialEq)]
pub enum IncDecTarget {
    Var(String),
    Field(Box<Expr>),
    Index { name: String, indices: Vec<Expr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// `^` / `**` — right-associative exponentiation (POSIX awk).
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Match,
    NotMatch,
    Concat,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    Not,
}

pub mod parallel;
