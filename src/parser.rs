use crate::ast::*;
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
        Expr::Index { name, index } => Ok(Expr::AssignIndex {
            name,
            index,
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
        }
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
            Token::End => {
                self.bump(true)?;
                Ok(Pattern::End)
            }
            Token::Regexp(s) => {
                let s = s.clone();
                self.bump(true)?;
                if self.cur == Token::Comma {
                    self.bump(false)?;
                    let p2 = self.parse_pattern()?;
                    return Ok(Pattern::Range(
                        Box::new(Pattern::Regexp(s)),
                        Box::new(p2),
                    ));
                }
                Ok(Pattern::Regexp(s))
            }
            Token::LBrace => Ok(Pattern::Empty),
            _ => {
                let e = self.parse_expr(false)?;
                if self.cur == Token::Comma {
                    self.bump(false)?;
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
                Ok(Stmt::If {
                    cond,
                    then_,
                    else_,
                })
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
                    let ix = self.parse_expr(false)?;
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
                        index: Some(ix),
                    })
                } else {
                    self.consume_stmt_end()?;
                    Ok(Stmt::Delete { name, index: None })
                }
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
                self.consume_stmt_end()?;
                Ok(Stmt::Print(args))
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
    fn parse_print_expr(&mut self) -> Result<Expr> {
        let mut e = self.parse_expr(false)?;
        loop {
            if matches!(
                self.cur,
                Token::Semi
                    | Token::Newline
                    | Token::Comma
                    | Token::RBrace
                    | Token::Eof
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
        loop {
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
            self.bump(false)?;
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
                    | Token::RBrace
                    | Token::RParen
                    | Token::RBracket
                    | Token::Colon
                    | Token::Eof
            ) {
                break;
            }
            // implicit concat: next token starts a new expr
            if matches!(
                self.cur,
                Token::Or | Token::And | Token::Eq | Token::Ne | Token::Lt | Token::Le
                    | Token::Gt | Token::Ge | Token::Tilde | Token::NotTilde | Token::Assign
                    | Token::AddAssign | Token::SubAssign | Token::MulAssign | Token::DivAssign
                    | Token::ModAssign | Token::Question
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
                    let ix = self.parse_expr(false)?;
                    if self.cur != Token::RBracket {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `]` after array index".into(),
                        });
                    }
                    self.bump(false)?;
                    Ok(Expr::Index {
                        name,
                        index: Box::new(ix),
                    })
                } else if self.cur == Token::LParen {
                    self.bump(false)?;
                    let mut args = Vec::new();
                    if self.cur != Token::RParen {
                        loop {
                            args.push(self.parse_expr(false)?);
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
                    let e = self.parse_expr(false)?;
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
                let e = self.parse_expr(false)?;
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
