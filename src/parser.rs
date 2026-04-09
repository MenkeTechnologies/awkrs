use crate::ast::{GetlineRedir, IncDecOp, IncDecTarget, *};
use crate::error::{Error, Result};
use crate::lexer::{Lexer, Token};
use std::collections::HashMap;

fn assign_expr(lhs: Expr, op: Option<BinOp>, rhs: Expr, line: usize) -> Result<Expr> {
    match lhs {
        Expr::Var(name) => Ok(Expr::Assign {
            name,
            op,
            rhs: Box::new(rhs),
        }),
        Expr::Field(inner) => Ok(Expr::AssignField {
            field: inner,
            op,
            rhs: Box::new(rhs),
        }),
        Expr::Index { name, indices } => Ok(Expr::AssignIndex {
            name,
            indices,
            op,
            rhs: Box::new(rhs),
        }),
        _ => Err(Error::Parse {
            line,
            msg: "invalid assignment target".into(),
        }),
    }
}

pub fn parse_program(src: &str) -> Result<Program> {
    let mut p = Parser::new(src);
    p.parse_program()
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    cur: Token,
    line: usize,
    /// When true, `>` / `>>` stay available for `print` redirection (not `a > b` comparison).
    in_print_arg: bool,
}

struct ParserCheckpoint<'a> {
    lexer: Lexer<'a>,
    cur: Token,
    line: usize,
}

const DOLLAR_FIELD_POSTFIX: &str = "__dollar_field_postfix__";

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        let mut lexer = Lexer::new(src);
        let cur = lexer.next_token(true).unwrap_or(Token::Eof);
        let line = lexer.line();
        Self {
            lexer,
            cur,
            line,
            in_print_arg: false,
        }
    }

    fn parse_expr_allow_gt(&mut self, regex_mode: bool) -> Result<Expr> {
        let saved = self.in_print_arg;
        self.in_print_arg = false;
        let e = self.parse_expr(regex_mode);
        self.in_print_arg = saved;
        e
    }

    fn bump(&mut self, regex_mode: bool) -> Result<()> {
        self.cur = self.lexer.next_token(regex_mode)?;
        self.line = self.lexer.line();
        Ok(())
    }

    fn skip_newlines(&mut self) -> Result<()> {
        while matches!(self.cur, Token::Newline) {
            self.bump(true)?;
        }
        Ok(())
    }

    fn checkpoint(&self) -> ParserCheckpoint<'a> {
        ParserCheckpoint {
            lexer: self.lexer.clone(),
            cur: self.cur.clone(),
            line: self.line,
        }
    }

    fn restore(&mut self, cp: ParserCheckpoint<'a>) {
        self.lexer = cp.lexer;
        self.cur = cp.cur;
        self.line = cp.line;
    }

    fn parse_program(&mut self) -> Result<Program> {
        let mut rules = Vec::new();
        let mut funcs = HashMap::new();
        self.skip_newlines()?;
        while !matches!(self.cur, Token::Eof) {
            if matches!(self.cur, Token::Function) {
                let f = self.parse_function_def()?;
                if funcs.insert(f.name.clone(), f).is_some() {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "duplicate function name".into(),
                    });
                }
            } else {
                rules.push(self.parse_rule()?);
            }
            self.skip_newlines()?;
        }
        Ok(Program { rules, funcs })
    }

    fn parse_function_def(&mut self) -> Result<FunctionDef> {
        self.bump(false)?;
        let Token::Ident(name) = &self.cur.clone() else {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected function name".into(),
            });
        };
        let name = name.clone();
        self.bump(false)?;
        if self.cur != Token::LParen {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected `(` after function name".into(),
            });
        }
        self.bump(false)?;
        let mut params = Vec::new();
        if self.cur != Token::RParen {
            loop {
                let Token::Ident(p) = &self.cur.clone() else {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected parameter name".into(),
                    });
                };
                params.push(p.clone());
                self.bump(false)?;
                if self.cur == Token::Comma {
                    self.bump(false)?;
                    continue;
                }
                break;
            }
        }
        if self.cur != Token::RParen {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected `)` after parameters".into(),
            });
        }
        self.bump(false)?;
        if self.cur != Token::LBrace {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected `{` before function body".into(),
            });
        }
        self.bump(false)?;
        let body = self.parse_stmt_list()?;
        if self.cur != Token::RBrace {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected `}` after function body".into(),
            });
        }
        self.bump(true)?;
        Ok(FunctionDef { name, params, body })
    }

    fn parse_rule(&mut self) -> Result<Rule> {
        let pattern = self.parse_pattern()?;
        self.skip_newlines()?;
        if self.cur != Token::LBrace {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected `{` after pattern".into(),
            });
        }
        self.bump(false)?;
        let stmts = self.parse_stmt_list()?;
        if self.cur != Token::RBrace {
            return Err(Error::Parse {
                line: self.line,
                msg: "expected `}`".into(),
            });
        }
        self.bump(true)?;
        Ok(Rule { pattern, stmts })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        match &self.cur.clone() {
            Token::Begin => {
                self.bump(true)?;
                Ok(Pattern::Begin)
            }
            Token::BeginFile => {
                self.bump(true)?;
                Ok(Pattern::BeginFile)
            }
            Token::End => {
                self.bump(true)?;
                Ok(Pattern::End)
            }
            Token::EndFile => {
                self.bump(true)?;
                Ok(Pattern::EndFile)
            }
            Token::Regexp(s) => {
                let s = s.clone();
                self.bump(true)?;
                if self.cur == Token::Comma {
                    // After `,`, the next pattern may start with `/…/` — needs regex lexer mode.
                    self.bump(true)?;
                    let p2 = self.parse_pattern()?;
                    return Ok(Pattern::Range(Box::new(Pattern::Regexp(s)), Box::new(p2)));
                }
                Ok(Pattern::Regexp(s))
            }
            Token::LBrace => Ok(Pattern::Empty),
            _ => {
                let e = self.parse_expr(false)?;
                if self.cur == Token::Comma {
                    self.bump(true)?;
                    let e2 = self.parse_expr(false)?;
                    Ok(Pattern::Range(
                        Box::new(Pattern::Expr(e)),
                        Box::new(Pattern::Expr(e2)),
                    ))
                } else {
                    Ok(Pattern::Expr(e))
                }
            }
        }
    }

    fn parse_stmt_list(&mut self) -> Result<Vec<Stmt>> {
        let mut v = Vec::new();
        self.skip_newlines()?;
        while self.cur != Token::RBrace && !matches!(self.cur, Token::Eof) {
            v.push(self.parse_stmt()?);
            self.skip_newlines()?;
        }
        Ok(v)
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        match &self.cur.clone() {
            Token::If => {
                self.bump(false)?;
                if self.cur != Token::LParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `(` after `if`".into(),
                    });
                }
                self.bump(false)?;
                let cond = self.parse_expr(false)?;
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)`".into(),
                    });
                }
                self.bump(false)?;
                let then_ = self.parse_stmt_block()?;
                let else_ = if matches!(self.cur, Token::Else) {
                    self.bump(false)?;
                    self.parse_stmt_block()?
                } else {
                    vec![]
                };
                Ok(Stmt::If { cond, then_, else_ })
            }
            Token::While => {
                self.bump(false)?;
                if self.cur != Token::LParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `(` after `while`".into(),
                    });
                }
                self.bump(false)?;
                let cond = self.parse_expr(false)?;
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)`".into(),
                    });
                }
                self.bump(false)?;
                let body = self.parse_stmt_block()?;
                Ok(Stmt::While { cond, body })
            }
            Token::Do => {
                self.bump(false)?;
                let body = self.parse_stmt_block()?;
                if self.cur != Token::While {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `while` after `do` body".into(),
                    });
                }
                self.bump(false)?;
                if self.cur != Token::LParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `(` after `while`".into(),
                    });
                }
                self.bump(false)?;
                let cond = self.parse_expr(false)?;
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)`".into(),
                    });
                }
                self.bump(false)?;
                if self.cur == Token::Semi || self.cur == Token::Newline {
                    self.bump(true)?;
                }
                Ok(Stmt::DoWhile { body, cond })
            }
            Token::For => {
                self.bump(false)?;
                if self.cur != Token::LParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `(` after `for`".into(),
                    });
                }
                self.bump(false)?;
                if let Token::Ident(var) = &self.cur.clone() {
                    let mut peek = self.lexer.clone();
                    if peek.next_token(false)? == Token::In {
                        let var = var.clone();
                        self.bump(false)?;
                        self.bump(false)?;
                        let Token::Ident(arr) = &self.cur.clone() else {
                            return Err(Error::Parse {
                                line: self.line,
                                msg: "expected array name in `for (x in a)`".into(),
                            });
                        };
                        let arr = arr.clone();
                        self.bump(false)?;
                        if self.cur != Token::RParen {
                            return Err(Error::Parse {
                                line: self.line,
                                msg: "expected `)`".into(),
                            });
                        }
                        self.bump(false)?;
                        let body = self.parse_stmt_block()?;
                        return Ok(Stmt::ForIn { var, arr, body });
                    }
                }
                let init = if self.cur == Token::Semi {
                    self.bump(false)?;
                    None
                } else {
                    let e = self.parse_expr(false)?;
                    if self.cur != Token::Semi {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `;` in `for`".into(),
                        });
                    }
                    self.bump(false)?;
                    Some(e)
                };
                let cond = if self.cur == Token::Semi {
                    self.bump(false)?;
                    None
                } else {
                    let e = self.parse_expr(false)?;
                    if self.cur != Token::Semi {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `;` in `for`".into(),
                        });
                    }
                    self.bump(false)?;
                    Some(e)
                };
                let iter = if self.cur == Token::RParen {
                    None
                } else {
                    let e = self.parse_expr(false)?;
                    Some(e)
                };
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)`".into(),
                    });
                }
                self.bump(false)?;
                let body = self.parse_stmt_block()?;
                Ok(Stmt::ForC {
                    init,
                    cond,
                    iter,
                    body,
                })
            }
            Token::Switch => {
                self.bump(false)?;
                if self.cur != Token::LParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `(` after `switch`".into(),
                    });
                }
                self.bump(false)?;
                let expr = self.parse_expr(false)?;
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)` after switch expression".into(),
                    });
                }
                self.bump(false)?;
                self.skip_newlines()?;
                if self.cur != Token::LBrace {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `{` after `switch (...)`".into(),
                    });
                }
                self.bump(false)?;
                let mut arms = Vec::new();
                loop {
                    self.skip_newlines()?;
                    if self.cur == Token::RBrace {
                        self.bump(false)?;
                        break;
                    }
                    match &self.cur {
                        Token::Case => {
                            // Regex mode so `case /pat/` yields `Token::Regexp`, not division `/`.
                            self.bump(true)?;
                            self.skip_newlines()?;
                            let label = match &self.cur {
                                Token::Regexp(s) => {
                                    let t = s.clone();
                                    self.bump(false)?;
                                    SwitchLabel::Regexp(t)
                                }
                                _ => {
                                    let e = self.parse_expr(false)?;
                                    SwitchLabel::Expr(e)
                                }
                            };
                            if self.cur != Token::Colon {
                                return Err(Error::Parse {
                                    line: self.line,
                                    msg: "expected `:` after `case` label".into(),
                                });
                            }
                            self.bump(false)?;
                            let stmts = self.parse_switch_case_stmts()?;
                            arms.push(SwitchArm::Case { label, stmts });
                        }
                        Token::Default => {
                            self.bump(false)?;
                            if self.cur != Token::Colon {
                                return Err(Error::Parse {
                                    line: self.line,
                                    msg: "expected `:` after `default`".into(),
                                });
                            }
                            self.bump(false)?;
                            let stmts = self.parse_switch_case_stmts()?;
                            arms.push(SwitchArm::Default { stmts });
                        }
                        _ => {
                            return Err(Error::Parse {
                                line: self.line,
                                msg: "expected `case`, `default`, or `}` in switch body".into(),
                            });
                        }
                    }
                }
                Ok(Stmt::Switch { expr, arms })
            }
            Token::Break => {
                self.bump(false)?;
                self.consume_stmt_end()?;
                Ok(Stmt::Break)
            }
            Token::Continue => {
                self.bump(false)?;
                self.consume_stmt_end()?;
                Ok(Stmt::Continue)
            }
            Token::Next => {
                self.bump(false)?;
                self.consume_stmt_end()?;
                Ok(Stmt::Next)
            }
            Token::NextFile => {
                self.bump(false)?;
                self.consume_stmt_end()?;
                Ok(Stmt::NextFile)
            }
            Token::Exit => {
                self.bump(false)?;
                let e = if matches!(
                    self.cur,
                    Token::Semi | Token::Newline | Token::RBrace | Token::Eof
                ) {
                    None
                } else {
                    Some(self.parse_expr(false)?)
                };
                self.consume_stmt_end()?;
                Ok(Stmt::Exit(e))
            }
            Token::Return => {
                self.bump(false)?;
                let e = if matches!(
                    self.cur,
                    Token::Semi | Token::Newline | Token::RBrace | Token::Eof
                ) {
                    None
                } else {
                    Some(self.parse_expr(false)?)
                };
                self.consume_stmt_end()?;
                Ok(Stmt::Return(e))
            }
            Token::Delete => {
                self.bump(false)?;
                let Token::Ident(name) = &self.cur.clone() else {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected array name after `delete`".into(),
                    });
                };
                let name = name.clone();
                self.bump(false)?;
                if self.cur == Token::LBracket {
                    self.bump(false)?;
                    let indices = self.parse_index_list()?;
                    if self.cur != Token::RBracket {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `]`".into(),
                        });
                    }
                    self.bump(false)?;
                    self.consume_stmt_end()?;
                    Ok(Stmt::Delete {
                        name,
                        indices: Some(indices),
                    })
                } else {
                    self.consume_stmt_end()?;
                    Ok(Stmt::Delete {
                        name,
                        indices: None,
                    })
                }
            }
            Token::Getline => {
                self.bump(false)?;
                let var = if let Token::Ident(name) = &self.cur.clone() {
                    let n = name.clone();
                    self.bump(false)?;
                    Some(n)
                } else {
                    None
                };
                if self.cur == Token::LtAmp {
                    self.bump(false)?;
                    let fe = self.parse_expr(false)?;
                    self.consume_stmt_end()?;
                    return Ok(Stmt::GetLine {
                        var,
                        redir: GetlineRedir::Coproc(Box::new(fe)),
                    });
                }
                if self.cur == Token::Lt {
                    self.bump(false)?;
                    let fe = self.parse_expr(false)?;
                    self.consume_stmt_end()?;
                    return Ok(Stmt::GetLine {
                        var,
                        redir: GetlineRedir::File(Box::new(fe)),
                    });
                }
                self.consume_stmt_end()?;
                Ok(Stmt::GetLine {
                    var,
                    redir: GetlineRedir::Primary,
                })
            }
            Token::LBrace => {
                self.bump(false)?;
                let b = self.parse_stmt_list()?;
                if self.cur != Token::RBrace {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `}`".into(),
                    });
                }
                self.bump(true)?;
                Ok(Stmt::Block(b))
            }
            Token::Print => {
                self.bump(false)?;
                let mut args = Vec::new();
                if matches!(
                    self.cur,
                    Token::Semi | Token::Newline | Token::RBrace | Token::Eof
                ) {
                    // empty print
                } else {
                    loop {
                        args.push(self.parse_print_expr()?);
                        if self.cur == Token::Comma {
                            self.bump(false)?;
                            continue;
                        }
                        break;
                    }
                }
                let redir = self.parse_print_redir()?;
                self.consume_stmt_end()?;
                Ok(Stmt::Print { args, redir })
            }
            Token::Printf => {
                self.bump(false)?;
                let mut args = Vec::new();
                if matches!(
                    self.cur,
                    Token::Semi | Token::Newline | Token::RBrace | Token::Eof
                ) {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "`printf` requires at least a format string".into(),
                    });
                }
                loop {
                    args.push(self.parse_print_expr()?);
                    if self.cur == Token::Comma {
                        self.bump(false)?;
                        continue;
                    }
                    break;
                }
                let redir = self.parse_print_redir()?;
                self.consume_stmt_end()?;
                Ok(Stmt::Printf { args, redir })
            }
            _ => {
                let e = self.parse_expr(false)?;
                self.consume_stmt_end()?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn parse_stmt_block(&mut self) -> Result<Vec<Stmt>> {
        if self.cur == Token::LBrace {
            self.bump(false)?;
            let b = self.parse_stmt_list()?;
            if self.cur != Token::RBrace {
                return Err(Error::Parse {
                    line: self.line,
                    msg: "expected `}`".into(),
                });
            }
            self.bump(true)?;
            Ok(b)
        } else {
            Ok(vec![self.parse_stmt()?])
        }
    }

    /// Statements inside a `switch` arm until the next `case` / `default` / `}`.
    fn parse_switch_case_stmts(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines()?;
            if matches!(self.cur, Token::Case | Token::Default | Token::RBrace) {
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn consume_stmt_end(&mut self) -> Result<()> {
        match self.cur {
            Token::Semi => {
                self.bump(true)?;
            }
            Token::Newline => {
                self.bump(true)?;
            }
            Token::RBrace | Token::Eof => {}
            _ => {
                return Err(Error::Parse {
                    line: self.line,
                    msg: "expected `;`, newline, or `}`".into(),
                });
            }
        }
        Ok(())
    }

    /// Inside `print`, space-separated items concatenate.
    fn parse_print_redir(&mut self) -> Result<Option<PrintRedir>> {
        match self.cur {
            Token::Gt => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Overwrite(Box::new(
                    self.parse_expr(false)?,
                ))))
            }
            Token::GtGt => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Append(Box::new(self.parse_expr(false)?))))
            }
            Token::Pipe => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Pipe(Box::new(self.parse_expr(false)?))))
            }
            Token::PipeCoproc => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Coproc(Box::new(self.parse_expr(false)?))))
            }
            _ => Ok(None),
        }
    }

    fn parse_print_expr(&mut self) -> Result<Expr> {
        let saved = self.in_print_arg;
        self.in_print_arg = true;
        let res = (|| -> Result<Expr> {
            let mut e = self.parse_expr(false)?;
            loop {
                if matches!(
                    self.cur,
                    Token::Semi
                        | Token::Newline
                        | Token::Comma
                        | Token::RBrace
                        | Token::Eof
                        | Token::Gt
                        | Token::GtGt
                        | Token::Pipe
                        | Token::PipeCoproc
                ) {
                    break;
                }
                let rhs = self.parse_expr(false)?;
                e = Expr::Binary {
                    op: BinOp::Concat,
                    left: Box::new(e),
                    right: Box::new(rhs),
                };
            }
            Ok(e)
        })();
        self.in_print_arg = saved;
        res
    }

    fn parse_expr(&mut self, regex_mode: bool) -> Result<Expr> {
        self.parse_assign(regex_mode)
    }

    fn parse_assign(&mut self, regex_mode: bool) -> Result<Expr> {
        let lhs = self.parse_cond(regex_mode)?;
        let op_tok = self.cur.clone();
        match op_tok {
            Token::Assign => {
                self.bump(false)?;
                let rhs = self.parse_assign(false)?;
                assign_expr(lhs, None, rhs, self.line)
            }
            Token::AddAssign
            | Token::SubAssign
            | Token::MulAssign
            | Token::DivAssign
            | Token::ModAssign => {
                let op = match op_tok {
                    Token::AddAssign => BinOp::Add,
                    Token::SubAssign => BinOp::Sub,
                    Token::MulAssign => BinOp::Mul,
                    Token::DivAssign => BinOp::Div,
                    Token::ModAssign => BinOp::Mod,
                    _ => unreachable!(),
                };
                self.bump(false)?;
                let rhs = self.parse_assign(false)?;
                assign_expr(lhs, Some(op), rhs, self.line)
            }
            _ => Ok(lhs),
        }
    }

    fn parse_cond(&mut self, regex_mode: bool) -> Result<Expr> {
        let e = self.parse_or(regex_mode)?;
        if self.cur == Token::Question {
            self.bump(false)?;
            let t = self.parse_expr(false)?;
            if self.cur != Token::Colon {
                return Err(Error::Parse {
                    line: self.line,
                    msg: "expected `:` in ternary".into(),
                });
            }
            self.bump(false)?;
            let f = self.parse_cond(false)?;
            return Ok(Expr::Ternary {
                cond: Box::new(e),
                then_: Box::new(t),
                else_: Box::new(f),
            });
        }
        Ok(e)
    }

    fn parse_or(&mut self, regex_mode: bool) -> Result<Expr> {
        let mut e = self.parse_and(regex_mode)?;
        while self.cur == Token::Or {
            self.bump(false)?;
            let r = self.parse_and(false)?;
            e = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_and(&mut self, regex_mode: bool) -> Result<Expr> {
        let mut e = self.parse_array(regex_mode)?;
        while self.cur == Token::And {
            self.bump(false)?;
            let r = self.parse_array(false)?;
            e = Expr::Binary {
                op: BinOp::And,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_array(&mut self, regex_mode: bool) -> Result<Expr> {
        self.parse_cmp(regex_mode)
    }

    fn parse_cmp(&mut self, regex_mode: bool) -> Result<Expr> {
        let mut e = self.parse_concat(regex_mode)?;
        if self.in_print_arg
            && matches!(
                self.cur,
                Token::Gt | Token::GtGt | Token::Pipe | Token::PipeCoproc
            )
        {
            return Ok(e);
        }
        loop {
            if self.cur == Token::In {
                self.bump(false)?;
                let Token::Ident(arr) = &self.cur.clone() else {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected array name after `in`".into(),
                    });
                };
                let arr = arr.clone();
                self.bump(false)?;
                e = Expr::In {
                    key: Box::new(e),
                    arr,
                };
                continue;
            }
            let op = match &self.cur {
                Token::Eq => Some(BinOp::Eq),
                Token::Ne => Some(BinOp::Ne),
                Token::Lt => Some(BinOp::Lt),
                Token::Le => Some(BinOp::Le),
                Token::Gt => Some(BinOp::Gt),
                Token::Ge => Some(BinOp::Ge),
                Token::Tilde => Some(BinOp::Match),
                Token::NotTilde => Some(BinOp::NotMatch),
                _ => None,
            };
            let Some(op) = op else { break };
            // RHS of `~` / `!~` may be `/regex/`; lexer must use regex mode for the next token.
            let regex_rhs = matches!(op, BinOp::Match | BinOp::NotMatch);
            self.bump(regex_rhs)?;
            let r = self.parse_concat(false)?;
            e = Expr::Binary {
                op,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_concat(&mut self, regex_mode: bool) -> Result<Expr> {
        let mut e = self.parse_additive(regex_mode)?;
        loop {
            if matches!(
                self.cur,
                Token::Semi
                    | Token::Newline
                    | Token::Comma
                    | Token::LBrace
                    | Token::RBrace
                    | Token::RParen
                    | Token::RBracket
                    | Token::Colon
                    | Token::Eof
                    | Token::Pipe
                    | Token::PipeCoproc
            ) {
                break;
            }
            // implicit concat: next token starts a new expr
            if matches!(
                self.cur,
                Token::Or
                    | Token::And
                    | Token::Eq
                    | Token::Ne
                    | Token::Lt
                    | Token::Le
                    | Token::Gt
                    | Token::GtGt
                    | Token::Ge
                    | Token::Tilde
                    | Token::NotTilde
                    | Token::Assign
                    | Token::AddAssign
                    | Token::SubAssign
                    | Token::MulAssign
                    | Token::DivAssign
                    | Token::ModAssign
                    | Token::Question
                    | Token::In
            ) {
                break;
            }
            let r = self.parse_additive(false)?;
            e = Expr::Binary {
                op: BinOp::Concat,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_additive(&mut self, regex_mode: bool) -> Result<Expr> {
        let mut e = self.parse_multiplicative(regex_mode)?;
        loop {
            let op = match &self.cur {
                Token::Plus => Some(BinOp::Add),
                Token::Minus => Some(BinOp::Sub),
                _ => None,
            };
            let Some(op) = op else { break };
            self.bump(false)?;
            let r = self.parse_multiplicative(false)?;
            e = Expr::Binary {
                op,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_multiplicative(&mut self, regex_mode: bool) -> Result<Expr> {
        let mut e = self.parse_unary(regex_mode)?;
        loop {
            let op = match &self.cur {
                Token::Star => Some(BinOp::Mul),
                Token::Slash => Some(BinOp::Div),
                Token::Percent => Some(BinOp::Mod),
                _ => None,
            };
            let Some(op) = op else { break };
            self.bump(false)?;
            let r = self.parse_unary(false)?;
            e = Expr::Binary {
                op,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_unary(&mut self, regex_mode: bool) -> Result<Expr> {
        let e = self.parse_prefix_unary(regex_mode)?;
        self.parse_postfix_on_expr(e)
    }

    /// Prefix unary operators; postfix `++`/`--` are handled by [`Self::parse_postfix_on_expr`].
    fn parse_prefix_unary(&mut self, regex_mode: bool) -> Result<Expr> {
        match &self.cur.clone() {
            Token::PlusPlus => {
                self.bump(false)?;
                let inner = self.parse_unary(false)?;
                Self::wrap_prefix_incdec(inner, IncDecOp::PreInc, self.line)
            }
            Token::MinusMinus => {
                self.bump(false)?;
                let inner = self.parse_unary(false)?;
                Self::wrap_prefix_incdec(inner, IncDecOp::PreDec, self.line)
            }
            Token::Bang => {
                self.bump(false)?;
                let e = self.parse_unary(false)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(e),
                })
            }
            Token::Minus => {
                self.bump(false)?;
                let e = self.parse_unary(false)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(e),
                })
            }
            Token::Plus => {
                self.bump(false)?;
                let e = self.parse_unary(false)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(e),
                })
            }
            _ => self.parse_primary(regex_mode),
        }
    }

    /// After `$` (not `$(`…`)`), parse the field index: `$i++` binds `++` to `i` before `$`;
    /// `$1++` binds `++` to the field as a whole.
    fn parse_inner_for_dollar_field(&mut self) -> Result<Expr> {
        let e = self.parse_prefix_unary(false)?;
        if matches!(e, Expr::Number(_) | Expr::Str(_))
            && matches!(self.cur, Token::PlusPlus | Token::MinusMinus)
        {
            return Err(Error::Parse {
                line: self.line,
                msg: DOLLAR_FIELD_POSTFIX.into(),
            });
        }
        self.parse_postfix_on_expr(e)
    }

    fn parse_postfix_on_expr(&mut self, mut e: Expr) -> Result<Expr> {
        loop {
            match &self.cur.clone() {
                Token::PlusPlus => {
                    self.bump(false)?;
                    let target = Self::expr_to_incdec_target(e, self.line)?;
                    e = Expr::IncDec {
                        op: IncDecOp::PostInc,
                        target,
                    };
                }
                Token::MinusMinus => {
                    self.bump(false)?;
                    let target = Self::expr_to_incdec_target(e, self.line)?;
                    e = Expr::IncDec {
                        op: IncDecOp::PostDec,
                        target,
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn wrap_prefix_incdec(inner: Expr, op: IncDecOp, line: usize) -> Result<Expr> {
        let target = Self::expr_to_incdec_target(inner, line)?;
        Ok(Expr::IncDec { op, target })
    }

    fn expr_to_incdec_target(e: Expr, line: usize) -> Result<IncDecTarget> {
        match e {
            Expr::Var(name) => Ok(IncDecTarget::Var(name)),
            Expr::Field(inner) => Ok(IncDecTarget::Field(inner)),
            Expr::Index { name, indices } => Ok(IncDecTarget::Index { name, indices }),
            _ => Err(Error::Parse {
                line,
                msg: "invalid `++`/`--` operand".into(),
            }),
        }
    }

    fn parse_index_list(&mut self) -> Result<Vec<Expr>> {
        let mut v = Vec::new();
        v.push(self.parse_expr_allow_gt(false)?);
        while self.cur == Token::Comma {
            self.bump(false)?;
            v.push(self.parse_expr_allow_gt(false)?);
        }
        Ok(v)
    }

    fn parse_primary(&mut self, _regex_mode: bool) -> Result<Expr> {
        match &self.cur.clone() {
            Token::Number(n) => {
                let n = *n;
                self.bump(false)?;
                Ok(Expr::Number(n))
            }
            Token::String(s) => {
                let s = s.clone();
                self.bump(false)?;
                Ok(Expr::Str(s))
            }
            Token::Regexp(s) => {
                let s = s.clone();
                self.bump(false)?;
                Ok(Expr::Str(s))
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.bump(false)?;
                if self.cur == Token::LBracket {
                    self.bump(false)?;
                    let indices = self.parse_index_list()?;
                    if self.cur != Token::RBracket {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `]` after array index".into(),
                        });
                    }
                    self.bump(false)?;
                    Ok(Expr::Index { name, indices })
                } else if self.cur == Token::LParen {
                    self.bump(false)?;
                    let mut args = Vec::new();
                    if self.cur != Token::RParen {
                        loop {
                            args.push(self.parse_expr_allow_gt(false)?);
                            if self.cur == Token::Comma {
                                self.bump(false)?;
                                continue;
                            }
                            break;
                        }
                    }
                    if self.cur != Token::RParen {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `)`".into(),
                        });
                    }
                    self.bump(false)?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Token::Dollar => {
                self.bump(false)?;
                if self.cur == Token::LParen {
                    self.bump(false)?;
                    let e = self.parse_expr_allow_gt(false)?;
                    if self.cur != Token::RParen {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `)` after `$(`".into(),
                        });
                    }
                    self.bump(false)?;
                    Ok(Expr::Field(Box::new(e)))
                } else {
                    let cp = self.checkpoint();
                    match self.parse_inner_for_dollar_field() {
                        Ok(inner) => Ok(Expr::Field(Box::new(inner))),
                        Err(Error::Parse { msg, .. }) if msg == DOLLAR_FIELD_POSTFIX => {
                            self.restore(cp);
                            let inner = self.parse_prefix_unary(false)?;
                            let e = Expr::Field(Box::new(inner));
                            self.parse_postfix_on_expr(e)
                        }
                        Err(e) => Err(e),
                    }
                }
            }
            Token::LParen => {
                self.bump(false)?;
                let e = self.parse_expr_allow_gt(false)?;
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)`".into(),
                    });
                }
                self.bump(false)?;
                Ok(e)
            }
            _ => Err(Error::Parse {
                line: self.line,
                msg: format!("unexpected token in expression: {:?}", self.cur),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, GetlineRedir, Pattern, PrintRedir, Stmt, SwitchArm, SwitchLabel};

    fn first_begin_stmt(prog: &crate::ast::Program) -> &Stmt {
        let rule = prog
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .expect("BEGIN rule");
        rule.stmts.first().expect("stmt")
    }

    #[test]
    fn parses_switch_numeric_and_regex_case_labels() {
        let p = parse_program(
            "BEGIN { switch (2) { case 1: break; case /foo/: break; default: break } }",
        )
        .unwrap();
        match first_begin_stmt(&p) {
            Stmt::Switch { expr, arms } => {
                assert!(matches!(expr, Expr::Number(n) if *n == 2.0));
                assert_eq!(arms.len(), 3);
                assert!(matches!(
                    &arms[0],
                    SwitchArm::Case {
                        label: SwitchLabel::Expr(Expr::Number(n)),
                        ..
                    } if *n == 1.0
                ));
                assert!(matches!(
                    &arms[1],
                    SwitchArm::Case {
                        label: SwitchLabel::Regexp(s),
                        ..
                    } if s == "foo"
                ));
                assert!(matches!(&arms[2], SwitchArm::Default { .. }));
            }
            _ => panic!("expected Switch"),
        }
    }

    #[test]
    fn parses_getline_coproc() {
        let p = parse_program("BEGIN { getline x <& \"cat\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::GetLine { var, redir } => {
                assert_eq!(var.as_deref(), Some("x"));
                assert!(matches!(redir, GetlineRedir::Coproc(_)));
            }
            _ => panic!("expected GetLine"),
        }
    }

    #[test]
    fn parses_print_coproc() {
        let p = parse_program("BEGIN { print \"y\" |& \"cat\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { redir, .. } => {
                assert!(matches!(redir, Some(PrintRedir::Coproc(_))));
            }
            _ => panic!("expected Print"),
        }
    }

    #[test]
    fn parses_printf_coproc() {
        let p = parse_program("BEGIN { printf \"%s\\n\", \"z\" |& \"cat\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Printf { redir, .. } => {
                assert!(matches!(redir, Some(PrintRedir::Coproc(_))));
            }
            _ => panic!("expected Printf"),
        }
    }

    #[test]
    fn parses_range_pattern_two_regexps() {
        let p = parse_program("/a/,/b/ { print 1 }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Range(_, _)))
            .expect("range rule");
        match &rule.pattern {
            Pattern::Range(b1, b2) => match (b1.as_ref(), b2.as_ref()) {
                (Pattern::Regexp(a), Pattern::Regexp(b)) => {
                    assert_eq!(a, "a");
                    assert_eq!(b, "b");
                }
                _ => panic!("expected two regexps"),
            },
            _ => panic!("expected range"),
        }
    }

    #[test]
    fn parses_match_expr_with_slash_regex() {
        parse_program("BEGIN { x = $0 ~ /z/ }").unwrap();
    }

    #[test]
    fn parses_in_operator_compared() {
        parse_program("BEGIN { x = (\"a\" in a) == 0 }").unwrap();
    }

    #[test]
    fn parses_in_operator() {
        let p = parse_program("BEGIN { print (\"k\" in a) }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .unwrap();
        match rule.stmts.first() {
            Some(Stmt::Print { args, .. }) => {
                assert_eq!(args.len(), 1);
                match &args[0] {
                    Expr::In { key, arr } => {
                        assert_eq!(arr, "a");
                        assert!(matches!(key.as_ref(), Expr::Str(s) if s == "k"));
                    }
                    _ => panic!("expected `in` expr"),
                }
            }
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn duplicate_function_name_errors() {
        let e = parse_program("function f(){return 1} function f(){return 2}").unwrap_err();
        match e {
            crate::error::Error::Parse { msg, .. } => {
                assert!(msg.contains("duplicate"), "{msg:?}");
            }
            e => panic!("unexpected err: {e:?}"),
        }
    }

    #[test]
    fn invalid_assignment_target_errors() {
        let e = parse_program("BEGIN { 1 = 2 }").unwrap_err();
        match e {
            crate::error::Error::Parse { msg, .. } => {
                assert!(msg.contains("assignment"), "{msg:?}");
            }
            e => panic!("unexpected err: {e:?}"),
        }
    }

    #[test]
    fn parses_empty_pattern_rule() {
        let p = parse_program("{ print 1 }").unwrap();
        assert_eq!(p.rules.len(), 1);
        assert!(matches!(p.rules[0].pattern, Pattern::Empty));
    }

    #[test]
    fn parses_function_with_params() {
        let p = parse_program("function sq(x){ return x*x } BEGIN { print sq(3) }").unwrap();
        let f = p.funcs.get("sq").expect("sq");
        assert_eq!(f.params, vec!["x".to_string()]);
    }

    #[test]
    fn parses_array_subscript_assign() {
        let p = parse_program("BEGIN { a[1] = 2 }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .unwrap();
        assert!(matches!(
            rule.stmts.first(),
            Some(Stmt::Expr(Expr::AssignIndex { .. }))
        ));
    }

    #[test]
    fn parses_print_redirect_file() {
        let p = parse_program("BEGIN { print \"hi\" > \"out.txt\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { redir, .. } => {
                assert!(matches!(redir, Some(PrintRedir::Overwrite(_))));
            }
            _ => panic!("expected Print"),
        }
    }

    #[test]
    fn parses_predefined_vars_in_expr() {
        parse_program("BEGIN { print NR, FNR, NF, FILENAME }").unwrap();
    }

    #[test]
    fn empty_source_yields_empty_program() {
        let p = parse_program("").unwrap();
        assert!(p.rules.is_empty());
        assert!(p.funcs.is_empty());
    }

    #[test]
    fn parse_error_unclosed_brace() {
        let e = parse_program("BEGIN { print 1").unwrap_err();
        assert!(matches!(e, crate::error::Error::Parse { .. }));
    }

    #[test]
    fn parse_error_invalid_expression() {
        let e = parse_program("BEGIN { + }").unwrap_err();
        assert!(matches!(e, crate::error::Error::Parse { .. }));
    }

    #[test]
    fn parses_break_continue_in_while() {
        parse_program("BEGIN { while (1) { break } }").unwrap();
        parse_program("BEGIN { while (1) { continue } }").unwrap();
    }

    #[test]
    fn parses_exit_with_code() {
        let p = parse_program("BEGIN { exit 5 }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .unwrap();
        assert!(matches!(rule.stmts.first(), Some(Stmt::Exit(Some(_)))));
    }

    #[test]
    fn parses_exit_default() {
        let p = parse_program("BEGIN { exit }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .unwrap();
        assert!(matches!(rule.stmts.first(), Some(Stmt::Exit(None))));
    }

    #[test]
    fn parses_printf_redirect_append() {
        let p = parse_program("BEGIN { printf \"%s\", \"a\" >> \"f\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Printf { redir, .. } => {
                assert!(matches!(redir, Some(PrintRedir::Append(_))));
            }
            _ => panic!("expected Printf"),
        }
    }

    #[test]
    fn parses_delete_entire_array_stmt() {
        let p = parse_program("BEGIN { delete a }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { name, indices } => {
                assert_eq!(name, "a");
                assert!(indices.is_none());
            }
            s => panic!("expected delete array, got {s:?}"),
        }
    }
}
