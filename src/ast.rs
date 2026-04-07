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
    Print(Vec<Expr>),
    Break,
    Continue,
    Next,
    Exit(Option<Expr>),
    Delete {
        name: String,
        index: Option<Expr>,
    },
    Return(Option<Expr>),
    /// `getline` / `getline var` / `getline < file` / `getline var < file`
    GetLine {
        var: Option<String>,
        redir: GetlineRedir,
    },
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
        index: Box<Expr>,
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
        index: Box<Expr>,
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
