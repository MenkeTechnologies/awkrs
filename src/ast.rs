//! Abstract syntax tree for awk programs (rules + optional user functions).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
/// `Program` — see fields for the structure layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// `rules` field.
    pub rules: Vec<Rule>,
    /// `funcs` field.
    pub funcs: HashMap<String, FunctionDef>,
}
/// `FunctionDef` — see fields for the structure layout.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    /// `name` field.
    pub name: String,
    /// `params` field.
    pub params: Vec<String>,
    /// `body` field.
    pub body: Vec<Stmt>,
}
/// `Rule` — see fields for the structure layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// `pattern` field.
    pub pattern: Pattern,
    /// `stmts` field.
    pub stmts: Vec<Stmt>,
}
/// `Pattern` — see variants for the choices.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `Begin` variant.
    Begin,
    /// `End` variant.
    End,
    /// gawk-style: run before each input file (after `BEGIN`).
    BeginFile,
    /// gawk-style: run after each input file (before `END`).
    EndFile,
    /// `Expr` variant.
    Expr(Expr),
    /// `Regexp` variant.
    Regexp(String),
    /// Inclusive range: two patterns (`/a/,/b/` or `NR==1,NR==5`).
    Range(Box<Pattern>, Box<Pattern>),
    /// `Empty` variant.
    Empty,
}
/// `Stmt` — see variants for the choices.
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
    /// `Block` variant.
    Block(Vec<Stmt>),
    /// `Expr` variant.
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
    /// `Break` variant.
    Break,
    /// `Continue` variant.
    Continue,
    /// `Next` variant.
    Next,
    /// Skip remaining records in the current input file (POSIX / gawk).
    NextFile,
    /// `Exit` variant.
    Exit(Option<Expr>),
    Delete {
        name: String,
        /// `None` = delete entire array; `Some(vec)` = delete one key (possibly multidimensional).
        indices: Option<Vec<Expr>>,
    },
    /// `Return` variant.
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
    /// `Expr` variant.
    Expr(Expr),
    /// `Regexp` variant.
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
/// `GetlineRedir` — see variants for the choices.
#[derive(Debug, Clone, PartialEq)]
pub enum GetlineRedir {
    /// Same stream as main input (or stdin).
    Primary,
    /// `getline ... < expr`
    File(Box<Expr>),
    /// `getline ... <& expr` — read from the stdout of the coprocess (same command string as `|&`).
    Coproc(Box<Expr>),
}
/// `Expr` — see variants for the choices.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `Number` variant.
    Number(f64),
    /// Decimal integer from source with no `.` — preserved as digits for **`-M`** (see [`crate::bytecode::Op::PushNumDecimalStr`]).
    IntegerLiteral(String),
    /// `Str` variant.
    Str(String),
    /// gawk-style regexp constant: `@/pattern/` — value type is **regexp**, not string (`typeof` is **`"regexp"`**).
    RegexpLiteral(String),
    /// `Var` variant.
    Var(String),
    /// `Field` variant.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncDecOp {
    /// `PreInc` variant.
    PreInc,
    /// `PostInc` variant.
    PostInc,
    /// `PreDec` variant.
    PreDec,
    /// `PostDec` variant.
    PostDec,
}

/// Lvalue for `++` / `--` only.
#[derive(Debug, Clone, PartialEq)]
pub enum IncDecTarget {
    /// `Var` variant.
    Var(String),
    /// `Field` variant.
    Field(Box<Expr>),
    Index {
        name: String,
        indices: Vec<Expr>,
    },
}
/// `BinOp` — see variants for the choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    /// `Add` variant.
    Add,
    /// `Sub` variant.
    Sub,
    /// `Mul` variant.
    Mul,
    /// `Div` variant.
    Div,
    /// `Mod` variant.
    Mod,
    /// `^` / `**` — right-associative exponentiation (POSIX awk).
    Pow,
    /// `Eq` variant.
    Eq,
    /// `Ne` variant.
    Ne,
    /// `Lt` variant.
    Lt,
    /// `Le` variant.
    Le,
    /// `Gt` variant.
    Gt,
    /// `Ge` variant.
    Ge,
    /// `Match` variant.
    Match,
    /// `NotMatch` variant.
    NotMatch,
    /// `Concat` variant.
    Concat,
    /// `And` variant.
    And,
    /// `Or` variant.
    Or,
}
/// `UnaryOp` — see variants for the choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `Neg` variant.
    Neg,
    /// `Pos` variant.
    Pos,
    /// `Not` variant.
    Not,
}
/// `parallel` submodule.
pub mod parallel;

#[cfg(test)]
mod ast_tests {
    use super::*;

    #[test]
    fn program_empty_clone_eq() {
        let p = Program {
            rules: vec![],
            funcs: HashMap::new(),
        };
        assert_eq!(p, p.clone());
    }

    #[test]
    fn pattern_range_holds_endpoints() {
        let p = Pattern::Range(
            Box::new(Pattern::Regexp("a".into())),
            Box::new(Pattern::Regexp("b".into())),
        );
        assert!(matches!(
            p,
            Pattern::Range(ref a, ref b)
                if matches!(**a, Pattern::Regexp(ref s) if s == "a")
                    && matches!(**b, Pattern::Regexp(ref s) if s == "b")
        ));
    }

    #[test]
    fn expr_clones_and_equality() {
        let e = Expr::Binary {
            op: BinOp::Add,
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::Var("x".into())),
        };
        assert_eq!(e, e.clone());
    }

    #[test]
    fn stmt_clones_and_equality() {
        let s = Stmt::If {
            cond: Expr::Number(1.0),
            then_: vec![Stmt::Break],
            else_: vec![Stmt::Continue],
        };
        assert_eq!(s, s.clone());
    }

    #[test]
    fn tuple_preserves_multiple_expressions() {
        let t = Expr::Tuple(vec![Expr::Number(1.0), Expr::Str("a".into())]);
        if let Expr::Tuple(ref v) = t {
            assert_eq!(v.len(), 2);
        } else {
            panic!("Expected tuple");
        }
    }

    #[test]
    fn ast_multidim_subscript_v2() {
        let e = Expr::Index {
            name: "a".into(),
            indices: vec![Expr::Number(1.0), Expr::Number(2.0)],
        };
        if let Expr::Index { indices, .. } = e {
            assert_eq!(indices.len(), 2);
        }
    }

    #[test]
    fn ast_delete_variants_v2() {
        let d1 = Stmt::Delete {
            name: "a".into(),
            indices: None,
        };
        let d2 = Stmt::Delete {
            name: "a".into(),
            indices: Some(vec![Expr::Number(1.0)]),
        };
        assert_ne!(d1, d2);
    }

    #[test]
    fn program_debug_v2() {
        let p = Program {
            rules: vec![],
            funcs: std::collections::HashMap::new(),
        };
        let s = format!("{:?}", p);
        assert!(s.contains("Program"));
    }

    #[test]
    fn binop_debug_v2() {
        assert_eq!(format!("{:?}", BinOp::Add), "Add");
    }

    #[test]
    fn binop_sub_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Sub), "Sub");
    }
    #[test]
    fn binop_mul_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Mul), "Mul");
    }
    #[test]
    fn binop_div_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Div), "Div");
    }
    #[test]
    fn binop_mod_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Mod), "Mod");
    }
    #[test]
    fn binop_pow_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Pow), "Pow");
    }
    #[test]
    fn binop_eq_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Eq), "Eq");
    }
    #[test]
    fn binop_ne_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Ne), "Ne");
    }
    #[test]
    fn binop_lt_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Lt), "Lt");
    }
    #[test]
    fn binop_le_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Le), "Le");
    }
    #[test]
    fn binop_gt_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Gt), "Gt");
    }
    #[test]
    fn binop_ge_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Ge), "Ge");
    }
    #[test]
    fn binop_match_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Match), "Match");
    }
    #[test]
    fn binop_notmatch_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::NotMatch), "NotMatch");
    }
    #[test]
    fn binop_concat_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Concat), "Concat");
    }
    #[test]
    fn binop_and_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::And), "And");
    }
    #[test]
    fn binop_or_debug_v3() {
        assert_eq!(format!("{:?}", BinOp::Or), "Or");
    }

    #[test]
    fn unaryop_neg_debug_v3() {
        assert_eq!(format!("{:?}", UnaryOp::Neg), "Neg");
    }
    #[test]
    fn unaryop_pos_debug_v3() {
        assert_eq!(format!("{:?}", UnaryOp::Pos), "Pos");
    }
    #[test]
    fn unaryop_not_debug_v3() {
        assert_eq!(format!("{:?}", UnaryOp::Not), "Not");
    }

    #[test]
    fn incdecop_preinc_debug_v3() {
        assert_eq!(format!("{:?}", IncDecOp::PreInc), "PreInc");
    }
    #[test]
    fn incdecop_postinc_debug_v3() {
        assert_eq!(format!("{:?}", IncDecOp::PostInc), "PostInc");
    }
    #[test]
    fn incdecop_predec_debug_v3() {
        assert_eq!(format!("{:?}", IncDecOp::PreDec), "PreDec");
    }
    #[test]
    fn incdecop_postdec_debug_v3() {
        assert_eq!(format!("{:?}", IncDecOp::PostDec), "PostDec");
    }

    #[test]
    fn getlinesource_primary_debug_v3() {
        assert!(format!("{:?}", GetlineRedir::Primary).contains("Primary"));
    }
    #[test]
    fn getlinesource_file_debug_v3() {
        assert!(format!("{:?}", GetlineRedir::File(Box::new(Expr::Number(1.0)))).contains("File"));
    }
    #[test]
    fn getlinesource_coproc_debug_v3() {
        assert!(
            format!("{:?}", GetlineRedir::Coproc(Box::new(Expr::Number(1.0)))).contains("Coproc")
        );
    }

    #[test]
    fn printredir_overwrite_debug_v3() {
        assert!(
            format!("{:?}", PrintRedir::Overwrite(Box::new(Expr::Number(1.0))))
                .contains("Overwrite")
        );
    }
    #[test]
    fn printredir_append_debug_v3() {
        assert!(
            format!("{:?}", PrintRedir::Append(Box::new(Expr::Number(1.0)))).contains("Append")
        );
    }
    #[test]
    fn printredir_pipe_debug_v3() {
        assert!(format!("{:?}", PrintRedir::Pipe(Box::new(Expr::Number(1.0)))).contains("Pipe"));
    }
    #[test]
    fn printredir_coproc_debug_v3() {
        assert!(
            format!("{:?}", PrintRedir::Coproc(Box::new(Expr::Number(1.0)))).contains("Coproc")
        );
    }

    #[test]
    fn stmt_break_debug_v3() {
        assert_eq!(format!("{:?}", Stmt::Break), "Break");
    }
    #[test]
    fn stmt_continue_debug_v3() {
        assert_eq!(format!("{:?}", Stmt::Continue), "Continue");
    }
    #[test]
    fn stmt_next_debug_v3() {
        assert_eq!(format!("{:?}", Stmt::Next), "Next");
    }
    #[test]
    fn stmt_nextfile_debug_v3() {
        assert_eq!(format!("{:?}", Stmt::NextFile), "NextFile");
    }

    #[test]
    fn ast_clones_batch_v7() {
        let e = Expr::Number(1.0);
        for _ in 0..30 {
            let _ = e.clone();
        }
    }

    #[test]
    fn ast_stmt_clones_batch_v7() {
        let s = Stmt::Break;
        for _ in 0..30 {
            let _ = s.clone();
        }
    }

    #[test]
    fn ast_expr_num_v8() {
        let _ = Expr::Number(0.0).clone();
    }
    #[test]
    fn ast_expr_num_v8_1() {
        let _ = Expr::Number(1.0).clone();
    }
    #[test]
    fn ast_expr_num_v8_2() {
        let _ = Expr::Number(-1.0).clone();
    }
    #[test]
    fn ast_expr_str_v8() {
        let _ = Expr::Str("".into()).clone();
    }
    #[test]
    fn ast_expr_str_v8_1() {
        let _ = Expr::Str("a".into()).clone();
    }
    #[test]
    fn ast_expr_regexp_v8() {
        let _ = Expr::RegexpLiteral("".into()).clone();
    }
    #[test]
    fn ast_expr_var_v8() {
        let _ = Expr::Var("x".into()).clone();
    }
    #[test]
    fn ast_expr_var_v8_1() {
        let _ = Expr::Var("y".into()).clone();
    }
    #[test]
    fn ast_expr_field_v8() {
        let _ = Expr::Field(Box::new(Expr::Number(0.0))).clone();
    }
    #[test]
    fn ast_expr_index_v8() {
        let _ = Expr::Index {
            name: "a".into(),
            indices: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_expr_call_v8() {
        let _ = Expr::Call {
            name: "f".into(),
            args: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_expr_unary_v8() {
        let _ = Expr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Number(0.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_binary_v8() {
        let _ = Expr::Binary {
            op: BinOp::Add,
            left: Box::new(Expr::Number(0.0)),
            right: Box::new(Expr::Number(0.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_assign_v8() {
        let _ = Expr::Assign {
            name: "x".into(),
            op: None,
            rhs: Box::new(Expr::Number(0.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_ternary_v8() {
        let _ = Expr::Ternary {
            cond: Box::new(Expr::Number(1.0)),
            then_: Box::new(Expr::Number(1.0)),
            else_: Box::new(Expr::Number(0.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_in_v8() {
        let _ = Expr::In {
            key: Box::new(Expr::Number(1.0)),
            arr: "a".into(),
        }
        .clone();
    }
    #[test]
    fn ast_expr_getline_v8() {
        let _ = Expr::GetLine {
            pipe_cmd: None,
            var: None,
            redir: GetlineRedir::Primary,
        }
        .clone();
    }
    #[test]
    fn ast_expr_incdec_v8() {
        let _ = Expr::IncDec {
            op: IncDecOp::PostInc,
            target: IncDecTarget::Var("x".into()),
        }
        .clone();
    }
    #[test]
    fn ast_expr_tuple_v8() {
        let _ = Expr::Tuple(vec![]).clone();
    }
    #[test]
    fn ast_expr_integerliteral_v8() {
        let _ = Expr::IntegerLiteral("1".into()).clone();
    }

    #[test]
    fn ast_stmt_if_v8() {
        let _ = Stmt::If {
            cond: Expr::Number(1.0),
            then_: vec![],
            else_: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_while_v8() {
        let _ = Stmt::While {
            cond: Expr::Number(1.0),
            body: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_for_v8() {
        let _ = Stmt::ForC {
            init: None,
            cond: None,
            iter: None,
            body: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_forin_v8() {
        let _ = Stmt::ForIn {
            var: "k".into(),
            arr: "a".into(),
            body: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_block_v8() {
        let _ = Stmt::Block(vec![]).clone();
    }
    #[test]
    fn ast_stmt_expr_v8() {
        let _ = Stmt::Expr(Expr::Number(1.0)).clone();
    }
    #[test]
    fn ast_stmt_print_v8() {
        let _ = Stmt::Print {
            args: vec![],
            redir: None,
        }
        .clone();
    }
    #[test]
    fn ast_stmt_printf_v8() {
        let _ = Stmt::Printf {
            args: vec![],
            redir: None,
        }
        .clone();
    }
    #[test]
    fn ast_stmt_exit_v8() {
        let _ = Stmt::Exit(None).clone();
    }
    #[test]
    fn ast_stmt_return_v8() {
        let _ = Stmt::Return(None).clone();
    }
    #[test]
    fn ast_stmt_delete_v8() {
        let _ = Stmt::Delete {
            name: "a".into(),
            indices: None,
        }
        .clone();
    }
    #[test]
    fn ast_stmt_switch_v8() {
        let _ = Stmt::Switch {
            expr: Expr::Number(1.0),
            arms: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_case_v8() {
        let _ = SwitchArm::Case {
            label: SwitchLabel::Expr(Expr::Number(1.0)),
            stmts: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_default_v8() {
        let _ = SwitchArm::Default { stmts: vec![] }.clone();
    }

    #[test]
    fn ast_expr_v53_0() {
        let _ = Expr::Number(1.0).clone();
    }
    #[test]
    fn ast_expr_v53_1() {
        let _ = Expr::Str("".into()).clone();
    }
    #[test]
    fn ast_expr_v53_2() {
        let _ = Expr::Var("x".into()).clone();
    }
    #[test]
    fn ast_expr_v53_3() {
        let _ = Expr::Field(Box::new(Expr::Number(1.0))).clone();
    }
    #[test]
    fn ast_expr_v53_4() {
        let _ = Expr::Index {
            name: "a".into(),
            indices: vec![Expr::Number(1.0)],
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_5() {
        let _ = Expr::Call {
            name: "f".into(),
            args: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_6() {
        let _ = Expr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Number(1.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_7() {
        let _ = Expr::Binary {
            op: BinOp::Add,
            left: Box::new(Expr::Number(1.0)),
            right: Box::new(Expr::Number(1.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_8() {
        let _ = Expr::Assign {
            name: "x".into(),
            op: None,
            rhs: Box::new(Expr::Number(1.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_9() {
        let _ = Expr::Ternary {
            cond: Box::new(Expr::Number(1.0)),
            then_: Box::new(Expr::Number(1.0)),
            else_: Box::new(Expr::Number(1.0)),
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_10() {
        let _ = Expr::In {
            key: Box::new(Expr::Number(1.0)),
            arr: "a".into(),
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_11() {
        let _ = Expr::GetLine {
            pipe_cmd: None,
            var: None,
            redir: GetlineRedir::Primary,
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_12() {
        let _ = Expr::IncDec {
            op: IncDecOp::PostInc,
            target: IncDecTarget::Var("x".into()),
        }
        .clone();
    }
    #[test]
    fn ast_expr_v53_13() {
        let _ = Expr::Tuple(vec![]).clone();
    }
    #[test]
    fn ast_expr_v53_14() {
        let _ = Expr::IntegerLiteral("1".into()).clone();
    }

    #[test]
    fn ast_stmt_v53_0() {
        let _ = Stmt::Break.clone();
    }
    #[test]
    fn ast_stmt_v53_1() {
        let _ = Stmt::Continue.clone();
    }
    #[test]
    fn ast_stmt_v53_2() {
        let _ = Stmt::Next.clone();
    }
    #[test]
    fn ast_stmt_v53_3() {
        let _ = Stmt::NextFile.clone();
    }
    #[test]
    fn ast_stmt_v53_4() {
        let _ = Stmt::Exit(None).clone();
    }
    #[test]
    fn ast_stmt_v53_5() {
        let _ = Stmt::Return(None).clone();
    }
    #[test]
    fn ast_stmt_v53_6() {
        let _ = Stmt::If {
            cond: Expr::Number(1.0),
            then_: vec![],
            else_: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_7() {
        let _ = Stmt::While {
            cond: Expr::Number(1.0),
            body: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_8() {
        let _ = Stmt::ForC {
            init: None,
            cond: None,
            iter: None,
            body: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_9() {
        let _ = Stmt::ForIn {
            var: "k".into(),
            arr: "a".into(),
            body: vec![],
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_10() {
        let _ = Stmt::Block(vec![]).clone();
    }
    #[test]
    fn ast_stmt_v53_11() {
        let _ = Stmt::Expr(Expr::Number(1.0)).clone();
    }
    #[test]
    fn ast_stmt_v53_12() {
        let _ = Stmt::Print {
            args: vec![],
            redir: None,
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_13() {
        let _ = Stmt::Printf {
            args: vec![],
            redir: None,
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_14() {
        let _ = Stmt::Delete {
            name: "a".into(),
            indices: None,
        }
        .clone();
    }
    #[test]
    fn ast_stmt_v53_15() {
        let _ = Stmt::Switch {
            expr: Expr::Number(1.0),
            arms: vec![],
        }
        .clone();
    }

    #[test]
    fn ast_pattern_v53_0() {
        let _ = Pattern::Begin.clone();
    }
    #[test]
    fn ast_pattern_v53_1() {
        let _ = Pattern::End.clone();
    }
    #[test]
    fn ast_pattern_v53_2() {
        let _ = Pattern::BeginFile.clone();
    }
    #[test]
    fn ast_pattern_v53_3() {
        let _ = Pattern::EndFile.clone();
    }
    #[test]
    fn ast_pattern_v53_4() {
        let _ = Pattern::Empty.clone();
    }
    #[test]
    fn ast_pattern_v53_5() {
        let _ = Pattern::Regexp("a".into()).clone();
    }
    #[test]
    fn ast_pattern_v53_6() {
        let _ = Pattern::Range(Box::new(Pattern::Begin), Box::new(Pattern::End)).clone();
    }
}
