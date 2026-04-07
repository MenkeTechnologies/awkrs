use crate::ast::{GetlineRedir, *};
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
                return assign_expr(lhs, None, rhs, self.line);
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
                return assign_expr(lhs, Some(op), rhs, self.line);
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
        match &self.cur {
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
                    let inner = self.parse_unary(false)?;
                    Ok(Expr::Field(Box::new(inner)))
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
    use crate::ast::{Expr, GetlineRedir, Pattern, PrintRedir, Stmt};

    fn first_begin_stmt(prog: &crate::ast::Program) -> &Stmt {
        let rule = prog
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .expect("BEGIN rule");
        rule.stmts.first().expect("stmt")
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
}
