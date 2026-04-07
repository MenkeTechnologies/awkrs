//! Abstract syntax tree for a minimal awk subset (extensible).

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub rules: Vec<Rule>,
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
    Range(Box<Expr>, Box<Expr>),
    Empty, // `{ ... }` matches every record
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
    Block(Vec<Stmt>),
    Expr(Expr),
    Print(Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Str(String),
    Var(String),
    Field(Box<Expr>),
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
        op: Option<BinOp>, // None for `=`, Some for `+=` etc.
        rhs: Box<Expr>,
    },
    AssignField {
        field: Box<Expr>,
        op: Option<BinOp>,
        rhs: Box<Expr>,
    },
    #[allow(dead_code)] // reserved for `++` / `--` in the lexer/parser
    Incr {
        name: String,
        delta: i8,
        pre: bool,
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
