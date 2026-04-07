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
    Exit(Option<Expr>),
    Delete {
        name: String,
        /// `None` = delete entire array; `Some(vec)` = delete one key (possibly multidimensional).
        indices: Option<Vec<Expr>>,
    },
    Return(Option<Expr>),
    /// `getline` / `getline var` / `getline < file` / `getline var < file`
    GetLine {
        var: Option<String>,
        redir: GetlineRedir,
    },
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum GetlineRedir {
    /// Same stream as main input (or stdin).
    Primary,
    /// `getline ... < expr`
    File(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Str(String),
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
    Ternary {
        cond: Box<Expr>,
        then_: Box<Expr>,
        else_: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
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
