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
        Expr::Tuple(_) => Err(Error::Parse {
            line,
            msg: "invalid assignment target".into(),
        }),
        _ => Err(Error::Parse {
            line,
            msg: "invalid assignment target".into(),
        }),
    }
}

/// Built-in arguments where bare `/re/` is a regexp pattern, not `$0 ~ /re/`.
fn builtin_regex_pattern_arg(fname: &str, arg_index: usize) -> bool {
    match fname {
        "gsub" | "sub" | "gensub" => arg_index == 0,
        "match" => arg_index == 1,
        "split" | "patsplit" => arg_index == 2,
        _ => false,
    }
}

pub fn parse_program(src: &str) -> Result<Program> {
    let expanded = crate::source_expand::expand_source_directives(src)?;
    let mut p = Parser::new(&expanded.text);
    let mut prog = p.parse_program()?;
    crate::namespace::apply_default_namespace(&mut prog, expanded.default_namespace.as_deref());
    Ok(prog)
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    cur: Token,
    line: usize,
    /// When true, `>` / `>>` stay available for `print` redirection (not `a > b` comparison).
    in_print_arg: bool,
    /// When parsing `printf` argument list — parenthesized comma lists are invalid (gawk).
    in_printf_args: bool,
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
            in_printf_args: false,
        }
    }

    fn reject_tuple_expr(e: &Expr, line: usize) -> Result<()> {
        if matches!(e, Expr::Tuple(_)) {
            return Err(Error::Parse {
                line,
                msg: "invalid use of parenthesized comma list".into(),
            });
        }
        Ok(())
    }

    fn parse_expr_allow_gt(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let saved = self.in_print_arg;
        self.in_print_arg = false;
        let e = self.parse_expr(regex_mode, re_pat);
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
        let stmts = if self.cur == Token::LBrace {
            self.bump(false)?;
            let stmts = self.parse_stmt_list()?;
            if self.cur != Token::RBrace {
                return Err(Error::Parse {
                    line: self.line,
                    msg: "expected `}`".into(),
                });
            }
            self.bump(true)?;
            stmts
        } else {
            match &pattern {
                // gawk: `BEGIN` / `END` / `BEGINFILE` / `ENDFILE` must have `{ … }` (not implicit `print $0`).
                Pattern::Begin | Pattern::End | Pattern::BeginFile | Pattern::EndFile => {
                    return Err(Error::Parse {
                        line: self.line,
                        msg:
                            "`BEGIN`, `END`, `BEGINFILE`, and `ENDFILE` require a `{ ... }` action"
                                .into(),
                    });
                }
                _ => {
                    // Record rule with no `{ … }` — POSIX default `{ print $0 }`.
                    vec![Stmt::Print {
                        args: vec![],
                        redir: None,
                    }]
                }
            }
        };
        Ok(Rule { pattern, stmts })
    }

    /// `/` was lexed as division [`Token::Slash`]; rewind and re-read as `/regex/` when it must
    /// start an expression (e.g. implicit concat in `print a /re/`).
    fn rescan_slash_as_regexp(&mut self) -> Result<()> {
        if self.cur == Token::Slash {
            self.lexer.rewind_slash_token();
            self.cur = self.lexer.next_token(true)?;
            self.line = self.lexer.line();
        }
        Ok(())
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
                self.skip_newlines()?;
                if self.pattern_regex_stands_alone() {
                    return Ok(Pattern::Regexp(s));
                }
                let e = self.parse_expr_from_concat_seed(Self::pattern_regex_match_seed(s))?;
                if self.cur == Token::Comma {
                    self.bump(true)?;
                    let e2 = self.parse_expr(false, false)?;
                    if matches!(e2, Expr::Tuple(_)) {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "parenthesized comma list cannot be a pattern".into(),
                        });
                    }
                    return Ok(Pattern::Range(
                        Box::new(Pattern::Expr(e)),
                        Box::new(Pattern::Expr(e2)),
                    ));
                }
                if matches!(e, Expr::Tuple(_)) {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "parenthesized comma list cannot be a pattern".into(),
                    });
                }
                Ok(Pattern::Expr(e))
            }
            Token::LBrace => Ok(Pattern::Empty),
            _ => {
                let e = self.parse_expr(false, false)?;
                if matches!(e, Expr::Tuple(_)) {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "parenthesized comma list cannot be a pattern".into(),
                    });
                }
                if self.cur == Token::Comma {
                    self.bump(true)?;
                    let e2 = self.parse_expr(false, false)?;
                    if matches!(e2, Expr::Tuple(_)) {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "parenthesized comma list cannot be a pattern".into(),
                        });
                    }
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
        self.skip_stmt_separators()?;
        while self.cur != Token::RBrace && !matches!(self.cur, Token::Eof) {
            v.push(self.parse_stmt()?);
            self.skip_stmt_separators()?;
        }
        Ok(v)
    }

    /// Skip any number of `;` and newline tokens — these all act as statement
    /// separators at the top of a `{ … }` block. gawk allows stray `;` as an
    /// empty statement (e.g. `if (cond) { … } ; print`); accepting them here
    /// stops `;` from being parsed as the start of an expression.
    fn skip_stmt_separators(&mut self) -> Result<()> {
        while matches!(self.cur, Token::Newline | Token::Semi) {
            self.bump(true)?;
        }
        Ok(())
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
                self.bump(true)?;
                let cond = self.parse_expr(false, false)?;
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
                self.bump(true)?;
                let cond = self.parse_expr(false, false)?;
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
                self.bump(true)?;
                let cond = self.parse_expr(false, false)?;
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
                self.bump(true)?;
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
                    let e = self.parse_expr(false, false)?;
                    if self.cur != Token::Semi {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `;` in `for`".into(),
                        });
                    }
                    self.bump(true)?;
                    Some(e)
                };
                let cond = if self.cur == Token::Semi {
                    self.bump(false)?;
                    None
                } else {
                    let e = self.parse_expr(false, false)?;
                    if self.cur != Token::Semi {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `;` in `for`".into(),
                        });
                    }
                    self.bump(true)?;
                    Some(e)
                };
                let iter = if self.cur == Token::RParen {
                    None
                } else {
                    let e = self.parse_expr(false, false)?;
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
                self.bump(true)?;
                let expr = self.parse_expr(false, false)?;
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
                                    let e = self.parse_expr(false, false)?;
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
                    Some(self.parse_expr(false, false)?)
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
                    Some(self.parse_expr(false, false)?)
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
                    self.bump(true)?;
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
                    let fe = self.parse_expr(false, false)?;
                    self.consume_stmt_end()?;
                    return Ok(Stmt::GetLine {
                        pipe_cmd: None,
                        var,
                        redir: GetlineRedir::Coproc(Box::new(fe)),
                    });
                }
                if self.cur == Token::Lt {
                    self.bump(false)?;
                    let fe = self.parse_expr(false, false)?;
                    self.consume_stmt_end()?;
                    return Ok(Stmt::GetLine {
                        pipe_cmd: None,
                        var,
                        redir: GetlineRedir::File(Box::new(fe)),
                    });
                }
                self.consume_stmt_end()?;
                Ok(Stmt::GetLine {
                    pipe_cmd: None,
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
                self.bump(true)?;
                let mut args = Vec::new();
                if matches!(
                    self.cur,
                    Token::Semi | Token::Newline | Token::RBrace | Token::Eof
                ) {
                    // empty print
                } else {
                    loop {
                        let arg = self.parse_print_expr()?;
                        if matches!(arg, Expr::Tuple(_)) && self.cur == Token::Comma {
                            return Err(Error::Parse {
                                line: self.line,
                                msg:
                                    "parenthesized comma list may not be followed by `,` in `print`"
                                        .into(),
                            });
                        }
                        args.push(arg);
                        if self.cur == Token::Comma {
                            self.bump(true)?;
                            // gawk parity: `,` is a continuation token — a
                            // newline after a comma is whitespace, not a
                            // statement terminator.
                            self.skip_newlines()?;
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
                self.bump(true)?;
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
                let saved_pf = self.in_printf_args;
                self.in_printf_args = true;
                let pf_res = loop {
                    let arg = self.parse_print_expr()?;
                    // gawk parity: `printf(fmt, a, b)` (function-call style) is
                    // equivalent to `printf fmt, a, b` (bare args). The parser
                    // sees the parenthesized comma list as `Expr::Tuple`; only
                    // unfold it when it is the ONLY arg (i.e. no bare commas
                    // follow). A bare-comma-after-tuple form like
                    // `printf (a,b), c` would mix paren-args with bare-args
                    // and is still rejected.
                    if matches!(arg, Expr::Tuple(_)) {
                        if !args.is_empty() || self.cur == Token::Comma {
                            break Err(Error::Parse {
                                line: self.line,
                                msg:
                                    "parenthesized comma list is not allowed in `printf` arguments"
                                        .into(),
                            });
                        }
                        if let Expr::Tuple(parts) = arg {
                            args.extend(parts);
                        }
                        break Ok(());
                    }
                    args.push(arg);
                    if self.cur == Token::Comma {
                        self.bump(true)?;
                        // `,` is a continuation token — newlines after a
                        // comma are whitespace.
                        self.skip_newlines()?;
                        continue;
                    }
                    break Ok(());
                };
                self.in_printf_args = saved_pf;
                pf_res?;
                let redir = self.parse_print_redir()?;
                self.consume_stmt_end()?;
                Ok(Stmt::Printf { args, redir })
            }
            _ => {
                let e = self.parse_expr(false, false)?;
                if matches!(e, Expr::Tuple(_)) {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "parenthesized comma list cannot be used as a statement".into(),
                    });
                }
                self.consume_stmt_end()?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn parse_stmt_block(&mut self) -> Result<Vec<Stmt>> {
        // POSIX / gawk: `if (cond) <NL> stmt`, `while (cond) <NL> stmt`,
        // `else <NL> stmt`, and similar control-flow bodies allow a newline
        // before the body. Skip leading newlines so the no-brace form parses.
        self.skip_newlines()?;
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
                    self.parse_expr(false, false)?,
                ))))
            }
            Token::GtGt => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Append(Box::new(
                    self.parse_expr(false, false)?,
                ))))
            }
            Token::Pipe => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Pipe(Box::new(
                    self.parse_expr(false, false)?,
                ))))
            }
            Token::PipeCoproc => {
                self.bump(false)?;
                Ok(Some(PrintRedir::Coproc(Box::new(
                    self.parse_expr(false, false)?,
                ))))
            }
            _ => Ok(None),
        }
    }

    fn parse_print_expr(&mut self) -> Result<Expr> {
        let saved = self.in_print_arg;
        self.in_print_arg = true;
        let res = (|| -> Result<Expr> {
            let mut e = self.parse_expr(false, false)?;
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
                let rhs = self.parse_expr(false, false)?;
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

    fn parse_expr(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let e = self.parse_assign(regex_mode, re_pat)?;
        self.parse_expr_pipe_getline_suffix(e)
    }

    /// After a `/regex/` token in a record rule, the pattern is a plain match unless another rule
    /// or `{` follows, or an operator continues a compound expression (`/foo/ && …`).
    fn pattern_regex_stands_alone(&self) -> bool {
        matches!(
            self.cur,
            Token::LBrace
                | Token::Semi
                | Token::Eof
                | Token::Begin
                | Token::End
                | Token::BeginFile
                | Token::EndFile
        ) || matches!(self.cur, Token::Regexp(_))
    }

    /// Same desugaring as [`Self::parse_primary`] for bare `/re/` in expression context:
    /// `$0 ~ "pattern"` (pattern stored as [`Expr::Str`] for the compiler/`~` pipeline).
    fn pattern_regex_match_seed(pat: String) -> Expr {
        Expr::Binary {
            op: BinOp::Match,
            left: Box::new(Expr::Field(Box::new(Expr::Number(0.0)))),
            right: Box::new(Expr::Str(pat)),
        }
    }

    /// Continue parsing from a completed [`Self::parse_concat`]-level subexpression (used when a
    /// record rule pattern starts with `/re/` but continues with `&&` / `||` / comparisons / …).
    fn parse_expr_from_concat_seed(&mut self, seed: Expr) -> Result<Expr> {
        let e = self.parse_cmp_rest(seed, false)?;
        let e = self.parse_and_rest(e, false)?;
        let e = self.parse_or_rest(e, false)?;
        let e = self.parse_cond_rest(e, false)?;
        let e = self.parse_assign_rest(e, false, false)?;
        self.parse_expr_pipe_getline_suffix(e)
    }

    fn parse_expr_pipe_getline_suffix(&mut self, e: Expr) -> Result<Expr> {
        // `expr | getline [var]` — pipe must be followed by `getline`.
        // `expr |& getline [var]` — gawk coprocess-read variant. awkrs's
        // current runtime treats `|&` as ordinary `|` for this expression form
        // (the bidirectional pipe with both write-from-print and read-from-
        // getline on the same coproc isn't implemented yet). At minimum the
        // parser must accept the syntax — otherwise scripts that use it error
        // before any work happens.
        if !matches!(self.cur, Token::Pipe | Token::PipeCoproc) {
            return Ok(e);
        }
        let mut peek = self.lexer.clone();
        if peek.next_token(false)? != Token::Getline {
            return Ok(e);
        }
        self.bump(false)?;
        self.bump(false)?;
        let var = if let Token::Ident(name) = &self.cur.clone() {
            let n = name.clone();
            self.bump(false)?;
            Some(n)
        } else {
            None
        };
        Ok(Expr::GetLine {
            pipe_cmd: Some(Box::new(e)),
            var,
            redir: GetlineRedir::Primary,
        })
    }

    fn parse_assign(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let lhs = self.parse_cond(regex_mode, re_pat)?;
        self.parse_assign_rest(lhs, regex_mode, re_pat)
    }

    fn parse_assign_rest(&mut self, lhs: Expr, _regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let op_tok = self.cur.clone();
        match op_tok {
            Token::Assign => {
                self.bump(false)?;
                let rhs = self.parse_assign(false, re_pat)?;
                assign_expr(lhs, None, rhs, self.line)
            }
            Token::AddAssign
            | Token::SubAssign
            | Token::MulAssign
            | Token::DivAssign
            | Token::ModAssign
            | Token::PowAssign => {
                let op = match op_tok {
                    Token::AddAssign => BinOp::Add,
                    Token::SubAssign => BinOp::Sub,
                    Token::MulAssign => BinOp::Mul,
                    Token::DivAssign => BinOp::Div,
                    Token::ModAssign => BinOp::Mod,
                    Token::PowAssign => BinOp::Pow,
                    _ => unreachable!(),
                };
                self.bump(false)?;
                let rhs = self.parse_assign(false, re_pat)?;
                assign_expr(lhs, Some(op), rhs, self.line)
            }
            _ => Ok(lhs),
        }
    }

    fn parse_cond(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let e = self.parse_or(regex_mode, re_pat)?;
        self.parse_cond_rest(e, re_pat)
    }

    fn parse_cond_rest(&mut self, mut e: Expr, re_pat: bool) -> Result<Expr> {
        if self.cur == Token::Question {
            Self::reject_tuple_expr(&e, self.line)?;
            self.bump(true)?;
            // gawk parity: `?` and `:` are statement-continuation tokens — a
            // newline after either is whitespace.
            self.skip_newlines()?;
            let t = self.parse_expr(false, re_pat)?;
            Self::reject_tuple_expr(&t, self.line)?;
            if self.cur != Token::Colon {
                return Err(Error::Parse {
                    line: self.line,
                    msg: "expected `:` in ternary".into(),
                });
            }
            self.bump(true)?;
            self.skip_newlines()?;
            // gawk parity: the else-branch is an assignment-expression (so
            // `1 ? x=1 : x=2` parses with `x=2` as the else arm). Previously
            // awkrs only allowed `conditional_expression` here, which made
            // `=2` look like a stray assign-into-ternary token. parse_assign
            // recursively handles nested ternary via parse_cond.
            let f = self.parse_assign(false, re_pat)?;
            Self::reject_tuple_expr(&f, self.line)?;
            e = Expr::Ternary {
                cond: Box::new(e),
                then_: Box::new(t),
                else_: Box::new(f),
            };
        }
        Ok(e)
    }

    fn parse_or(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let e = self.parse_and(regex_mode, re_pat)?;
        self.parse_or_rest(e, re_pat)
    }

    fn parse_or_rest(&mut self, mut e: Expr, re_pat: bool) -> Result<Expr> {
        while self.cur == Token::Or {
            Self::reject_tuple_expr(&e, self.line)?;
            self.bump(true)?;
            // gawk parity: `||` is a continuation token; newlines after it are
            // treated as whitespace.
            self.skip_newlines()?;
            let r = self.parse_and(false, re_pat)?;
            Self::reject_tuple_expr(&r, self.line)?;
            e = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_and(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let e = self.parse_array(regex_mode, re_pat)?;
        self.parse_and_rest(e, re_pat)
    }

    fn parse_and_rest(&mut self, mut e: Expr, re_pat: bool) -> Result<Expr> {
        while self.cur == Token::And {
            Self::reject_tuple_expr(&e, self.line)?;
            self.bump(true)?;
            // `&&` is a continuation token.
            self.skip_newlines()?;
            let r = self.parse_array(false, re_pat)?;
            Self::reject_tuple_expr(&r, self.line)?;
            e = Expr::Binary {
                op: BinOp::And,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_array(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        self.parse_cmp(regex_mode, re_pat)
    }

    fn parse_cmp(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let e = self.parse_concat(regex_mode, re_pat)?;
        if self.in_print_arg
            && matches!(
                self.cur,
                Token::Gt | Token::GtGt | Token::Pipe | Token::PipeCoproc
            )
        {
            return Ok(e);
        }
        self.parse_cmp_rest(e, re_pat)
    }

    fn parse_cmp_rest(&mut self, mut e: Expr, _re_pat: bool) -> Result<Expr> {
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
            Self::reject_tuple_expr(&e, self.line)?;
            // RHS of `~` / `!~` may be `/regex/`; lexer must use regex mode for the next token.
            let regex_rhs = matches!(op, BinOp::Match | BinOp::NotMatch);
            self.bump(regex_rhs)?;
            // Bare `/re/` here is the pattern operand only (not `$0 ~ /re/`).
            let r = self.parse_concat(false, true)?;
            Self::reject_tuple_expr(&r, self.line)?;
            e = Expr::Binary {
                op,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_concat(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let mut e = self.parse_additive(regex_mode, re_pat)?;
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
                    | Token::PowAssign
                    | Token::Question
                    | Token::In
                    | Token::Caret
                    | Token::StarStar
            ) {
                break;
            }
            let r = self.parse_additive(false, re_pat)?;
            Self::reject_tuple_expr(&e, self.line)?;
            Self::reject_tuple_expr(&r, self.line)?;
            e = Expr::Binary {
                op: BinOp::Concat,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_additive(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let mut e = self.parse_multiplicative(regex_mode, re_pat)?;
        loop {
            let op = match &self.cur {
                Token::Plus => Some(BinOp::Add),
                Token::Minus => Some(BinOp::Sub),
                _ => None,
            };
            let Some(op) = op else { break };
            Self::reject_tuple_expr(&e, self.line)?;
            self.bump(true)?;
            let r = self.parse_multiplicative(false, re_pat)?;
            Self::reject_tuple_expr(&r, self.line)?;
            e = Expr::Binary {
                op,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    fn parse_multiplicative(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let mut e = self.parse_unary(regex_mode, re_pat)?;
        loop {
            let op = match &self.cur {
                Token::Star => Some(BinOp::Mul),
                Token::Slash => Some(BinOp::Div),
                Token::Percent => Some(BinOp::Mod),
                _ => None,
            };
            let Some(op) = op else { break };
            Self::reject_tuple_expr(&e, self.line)?;
            self.bump(true)?;
            let r = self.parse_unary(false, re_pat)?;
            Self::reject_tuple_expr(&r, self.line)?;
            e = Expr::Binary {
                op,
                left: Box::new(e),
                right: Box::new(r),
            };
        }
        Ok(e)
    }

    /// Prefix `++`/`--` bind tighter than `^` / `**`; `!` / unary `+` / `-` bind looser than `^`
    /// (e.g. `-2^2` is `-(2^2)`).  `!` / `+` / `-` live in [`Self::parse_prefix_unary`] so `$-1`
    /// works without consuming `$1++`-style postfix too early.
    fn parse_unary(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        match &self.cur.clone() {
            Token::PlusPlus => {
                self.bump(false)?;
                let inner = self.parse_unary(false, re_pat)?;
                Self::wrap_prefix_incdec(inner, IncDecOp::PreInc, self.line)
            }
            Token::MinusMinus => {
                self.bump(false)?;
                let inner = self.parse_unary(false, re_pat)?;
                Self::wrap_prefix_incdec(inner, IncDecOp::PreDec, self.line)
            }
            _ => self.parse_power(regex_mode, re_pat),
        }
    }

    /// `^` / `**` — right-associative; postfix `++`/`--` are handled before `^` (e.g. `x++^2`).
    fn parse_power(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        let mut e = self.parse_prefix_unary(regex_mode, re_pat)?;
        e = self.parse_postfix_on_expr(e)?;
        if matches!(self.cur, Token::Caret | Token::StarStar) {
            Self::reject_tuple_expr(&e, self.line)?;
            self.bump(true)?;
            let rhs = self.parse_power(false, re_pat)?;
            Self::reject_tuple_expr(&rhs, self.line)?;
            return Ok(Expr::Binary {
                op: BinOp::Pow,
                left: Box::new(e),
                right: Box::new(rhs),
            });
        }
        Ok(e)
    }

    /// Prefix unary operators; postfix `++`/`--` are handled by [`Self::parse_postfix_on_expr`].
    fn parse_prefix_unary(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        match &self.cur.clone() {
            Token::PlusPlus => {
                self.bump(false)?;
                let inner = self.parse_unary(false, re_pat)?;
                Self::wrap_prefix_incdec(inner, IncDecOp::PreInc, self.line)
            }
            Token::MinusMinus => {
                self.bump(false)?;
                let inner = self.parse_unary(false, re_pat)?;
                Self::wrap_prefix_incdec(inner, IncDecOp::PreDec, self.line)
            }
            Token::Bang => {
                self.bump(true)?;
                let e = self.parse_power(false, re_pat)?;
                Self::reject_tuple_expr(&e, self.line)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(e),
                })
            }
            Token::Minus => {
                self.bump(true)?;
                let e = self.parse_power(false, re_pat)?;
                Self::reject_tuple_expr(&e, self.line)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(e),
                })
            }
            Token::Plus => {
                self.bump(true)?;
                let e = self.parse_power(false, re_pat)?;
                Self::reject_tuple_expr(&e, self.line)?;
                Ok(Expr::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(e),
                })
            }
            _ => self.parse_primary(regex_mode, re_pat),
        }
    }

    /// After `$` (not `$(`…`)`), parse the field index: `$i++` binds `++` to `i` before `$`;
    /// `$1++` binds `++` to the field as a whole.
    fn parse_inner_for_dollar_field(&mut self) -> Result<Expr> {
        // Prefix (including unary `-` for `$-1`) then postfix; bare `$1++` uses the error path in
        // `Token::Dollar` to attach `++` to the field, not to the integer.
        let e = self.parse_prefix_unary(false, false)?;
        if matches!(e, Expr::Number(_) | Expr::IntegerLiteral(_) | Expr::Str(_))
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
        v.push(self.parse_expr_allow_gt(false, false)?);
        while self.cur == Token::Comma {
            self.bump(true)?;
            self.skip_newlines()?;
            v.push(self.parse_expr_allow_gt(false, false)?);
        }
        Ok(v)
    }

    fn parse_primary(&mut self, regex_mode: bool, re_pat: bool) -> Result<Expr> {
        // `/` is only lexed as [`Token::Slash`] when `regex_mode` was false (e.g. after `=`).
        // At the start of a **primary**, `/` cannot be division — division is handled in
        // [`Self::parse_multiplicative`]. Reinterpret as `/regex/` (POSIX awk).
        //
        // Do **not** set `regex_mode = true` for whole `gsub`/`sub`/… arguments: a replacement
        // like `b/c` must stay division; only this primary-boundary rule is correct.
        let _ = regex_mode;
        if self.cur == Token::Slash {
            self.rescan_slash_as_regexp()?;
        }
        match &self.cur.clone() {
            Token::Number(n) => {
                let n = *n;
                self.bump(false)?;
                Ok(Expr::Number(n))
            }
            Token::IntegerLiteral(s) => {
                let s = s.clone();
                self.bump(false)?;
                Ok(Expr::IntegerLiteral(s))
            }
            Token::String(s) => {
                let s = s.clone();
                self.bump(false)?;
                Ok(Expr::Str(s))
            }
            Token::Regexp(s) => {
                let s = s.clone();
                self.bump(false)?;
                if re_pat {
                    Ok(Expr::Str(s))
                } else {
                    Ok(Expr::Binary {
                        op: BinOp::Match,
                        left: Box::new(Expr::Field(Box::new(Expr::Number(0.0)))),
                        right: Box::new(Expr::Str(s)),
                    })
                }
            }
            Token::At => {
                // `@/re/` regexp constant (gawk) vs `@var(...)` indirect call.
                let checkpoint = self.checkpoint();
                self.bump(true)?;
                if let Token::Regexp(s) = &self.cur {
                    let s = s.clone();
                    self.bump(false)?;
                    return Ok(Expr::RegexpLiteral(s));
                }
                self.restore(checkpoint);
                self.bump(false)?;
                // gawk parity: `@<expr>(args)`. The callee is a *string-valued*
                // expression — usually a bare identifier (variable holding the
                // function name) but `gawk` also accepts more general primaries.
                // Parse just the variable / field / indexed-element form here so
                // we don't consume the following `(args)` as if it were a call
                // on the callee itself.
                let callee = match &self.cur.clone() {
                    Token::Ident(name) => {
                        let name = name.clone();
                        self.bump(false)?;
                        // Accept `@a[k](...)` too — the callee can be an array element.
                        if self.cur == Token::LBracket {
                            self.bump(true)?;
                            let indices = self.parse_index_list()?;
                            if self.cur != Token::RBracket {
                                return Err(Error::Parse {
                                    line: self.line,
                                    msg: "expected `]`".into(),
                                });
                            }
                            self.bump(false)?;
                            Expr::Index { name, indices }
                        } else {
                            Expr::Var(name)
                        }
                    }
                    Token::Dollar => {
                        // `@$1(args)` — callee read from a field.
                        self.bump(false)?;
                        let inner = self.parse_primary(false, false)?;
                        Expr::Field(Box::new(inner))
                    }
                    Token::LParen => {
                        // Parenthesized expression callee.
                        self.bump(true)?;
                        let inner = self.parse_expr_allow_gt(false, false)?;
                        if self.cur != Token::RParen {
                            return Err(Error::Parse {
                                line: self.line,
                                msg: "expected `)` around `@<expr>` callee".into(),
                            });
                        }
                        self.bump(false)?;
                        inner
                    }
                    _ => {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected variable, `$<field>`, or `(expr)` after `@` for \
                                  indirect call"
                                .into(),
                        });
                    }
                };
                if self.cur != Token::LParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `(` after `@` expression for indirect call".into(),
                    });
                }
                self.bump(true)?;
                let mut args = Vec::new();
                if self.cur != Token::RParen {
                    loop {
                        args.push(self.parse_expr_allow_gt(false, false)?);
                        if self.cur == Token::Comma {
                            self.bump(true)?;
                            self.skip_newlines()?;
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
                Ok(Expr::IndirectCall {
                    callee: Box::new(callee),
                    args,
                })
            }
            Token::Ident(name) => {
                let name = name.clone();
                self.bump(false)?;
                if self.cur == Token::LBracket {
                    self.bump(true)?;
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
                    self.bump(true)?;
                    let mut args = Vec::new();
                    if self.cur != Token::RParen {
                        loop {
                            let re_for_arg = builtin_regex_pattern_arg(&name, args.len());
                            args.push(self.parse_expr_allow_gt(false, re_for_arg)?);
                            if self.cur == Token::Comma {
                                self.bump(true)?;
                                self.skip_newlines()?;
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
                    // POSIX: `length` with no `(` means `length($0)` (same as `length()`).
                    if name == "length" {
                        Ok(Expr::Call {
                            name,
                            args: Vec::new(),
                        })
                    } else {
                        Ok(Expr::Var(name))
                    }
                }
            }
            Token::Dollar => {
                self.bump(false)?;
                if self.cur == Token::LParen {
                    self.bump(true)?;
                    let e = self.parse_expr_allow_gt(false, re_pat)?;
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
                            let inner = self.parse_prefix_unary(false, false)?;
                            let e = Expr::Field(Box::new(inner));
                            self.parse_postfix_on_expr(e)
                        }
                        Err(e) => Err(e),
                    }
                }
            }
            Token::LParen => {
                self.bump(true)?;
                let first = self.parse_expr_allow_gt(false, re_pat)?;
                if self.cur == Token::Comma {
                    // gawk parity: `printf("fmt", a, b)` is the function-call
                    // form of `printf "fmt", a, b`. Earlier awkrs rejected the
                    // parenthesized comma list when nested inside printf args;
                    // we now build the `Expr::Tuple` and the printf statement
                    // parser unfolds it (still rejects mixed paren-args + bare
                    // args like `printf (a,b), c`).
                    let mut parts = vec![first];
                    while self.cur == Token::Comma {
                        self.bump(true)?;
                        parts.push(self.parse_expr_allow_gt(false, re_pat)?);
                    }
                    if self.cur != Token::RParen {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "expected `)`".into(),
                        });
                    }
                    self.bump(false)?;
                    for p in &parts {
                        if matches!(p, Expr::Tuple(_)) {
                            return Err(Error::Parse {
                                line: self.line,
                                msg: "nested parenthesized comma lists are not allowed".into(),
                            });
                        }
                    }
                    return Ok(Expr::Tuple(parts));
                }
                if self.cur != Token::RParen {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected `)`".into(),
                    });
                }
                self.bump(false)?;
                Ok(first)
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
                    let fe = self.parse_expr(false, false)?;
                    return Ok(Expr::GetLine {
                        pipe_cmd: None,
                        var,
                        redir: GetlineRedir::Coproc(Box::new(fe)),
                    });
                }
                if self.cur == Token::Lt {
                    self.bump(false)?;
                    let fe = self.parse_expr(false, false)?;
                    return Ok(Expr::GetLine {
                        pipe_cmd: None,
                        var,
                        redir: GetlineRedir::File(Box::new(fe)),
                    });
                }
                Ok(Expr::GetLine {
                    pipe_cmd: None,
                    var,
                    redir: GetlineRedir::Primary,
                })
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
    use crate::ast::{
        BinOp, Expr, GetlineRedir, IncDecOp, IncDecTarget, Pattern, PrintRedir, Rule, Stmt,
        SwitchArm, SwitchLabel, UnaryOp,
    };

    fn first_begin_stmt(prog: &crate::ast::Program) -> &Stmt {
        let rule = prog
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .expect("BEGIN rule");
        rule.stmts.first().expect("stmt")
    }

    /// Decimal integers without `.` become [`Expr::IntegerLiteral`]; tests that used to expect [`Expr::Number`] accept both.
    fn expr_is_int(e: &Expr, v: i64) -> bool {
        match e {
            Expr::IntegerLiteral(s) => s.parse::<i64>().ok() == Some(v),
            Expr::Number(n) => *n == v as f64,
            _ => false,
        }
    }

    #[test]
    fn parses_power_right_associative_star_star() {
        let p = parse_program("BEGIN { x = 2 ** 3 ** 2 }").unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        let Expr::Binary { op, left, right } = rhs.as_ref() else {
            panic!("expected binary, got {rhs:?}");
        };
        assert_eq!(*op, BinOp::Pow);
        assert!(expr_is_int(left, 2));
        let Expr::Binary {
            op: iop,
            left: il,
            right: ir,
        } = right.as_ref()
        else {
            panic!("expected nested ** on rhs");
        };
        assert_eq!(*iop, BinOp::Pow);
        assert!(expr_is_int(il, 3));
        assert!(expr_is_int(ir, 2));
    }

    #[test]
    fn parses_unary_minus_binds_outside_power() {
        let p = parse_program("BEGIN { x = -2 ^ 2 }").unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        let Expr::Unary { op, expr } = rhs.as_ref() else {
            panic!("expected unary, got {rhs:?}");
        };
        assert_eq!(*op, UnaryOp::Neg);
        let Expr::Binary { op: bop, .. } = expr.as_ref() else {
            panic!("expected pow inside unary, got {expr:?}");
        };
        assert_eq!(*bop, BinOp::Pow);
    }

    #[test]
    fn parses_compound_add_assign_statement() {
        let p = parse_program("BEGIN { x += 1 }").unwrap();
        let Stmt::Expr(Expr::Assign { name, op, rhs }) = first_begin_stmt(&p) else {
            panic!("expected assign expr stmt");
        };
        assert_eq!(name, "x");
        assert_eq!(*op, Some(BinOp::Add));
        assert!(expr_is_int(rhs, 1), "rhs={rhs:?}");
    }

    #[test]
    fn parses_compound_mul_and_sub_assign() {
        for (src, expected_op) in [
            ("BEGIN { y *= 2 }", BinOp::Mul),
            ("BEGIN { z -= 3 }", BinOp::Sub),
        ] {
            let p = parse_program(src).unwrap();
            let Stmt::Expr(Expr::Assign { op, .. }) = first_begin_stmt(&p) else {
                panic!("{src}: expected assign");
            };
            assert_eq!(*op, Some(expected_op), "{src}");
        }
    }

    #[test]
    fn parses_compound_div_assign() {
        let p = parse_program("BEGIN { q /= 4 }").unwrap();
        let Stmt::Expr(Expr::Assign { op, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        assert_eq!(*op, Some(BinOp::Div));
    }

    #[test]
    fn parses_compound_mod_assign() {
        let p = parse_program("BEGIN { m %= 5 }").unwrap();
        let Stmt::Expr(Expr::Assign { op, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        assert_eq!(*op, Some(BinOp::Mod));
    }

    #[test]
    fn parses_adjacent_string_literals_implicit_concat() {
        let p = parse_program(r#"BEGIN { x = "foo" "bar" }"#).unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        let Expr::Binary { op, left, right } = rhs.as_ref() else {
            panic!("expected concat, got {rhs:?}");
        };
        assert_eq!(*op, BinOp::Concat);
        assert!(matches!(left.as_ref(), Expr::Str(s) if s == "foo"));
        assert!(matches!(right.as_ref(), Expr::Str(s) if s == "bar"));
    }

    #[test]
    fn parses_three_adjacent_string_literals_nested_concat() {
        let p = parse_program(r#"BEGIN { x = "a" "b" "c" }"#).unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        let Expr::Binary {
            op: o1,
            left,
            right,
        } = rhs.as_ref()
        else {
            panic!("expected binary");
        };
        assert_eq!(*o1, BinOp::Concat);
        let Expr::Binary {
            op: o2,
            left: l2,
            right: r2,
        } = left.as_ref()
        else {
            panic!("expected nested concat on left");
        };
        assert_eq!(*o2, BinOp::Concat);
        assert!(matches!(l2.as_ref(), Expr::Str(s) if s == "a"));
        assert!(matches!(r2.as_ref(), Expr::Str(s) if s == "b"));
        assert!(matches!(right.as_ref(), Expr::Str(s) if s == "c"));
    }

    #[test]
    fn gsub_sub_split_accept_regexp_literal_args() {
        for prog in [
            r#"BEGIN { gsub(/a/, "X", s) }"#,
            r#"BEGIN { sub(/a/, "X", s) }"#,
            r#"BEGIN { split("a1b", a, /[0-9]/) }"#,
            // RHS of `=` must lex `/` as regexp, not division (see `bump` after `=`).
            r#"BEGIN { x = /foo/ }"#,
        ] {
            parse_program(prog).unwrap_or_else(|e| panic!("parse {prog:?}: {e:?}"));
        }
    }

    #[test]
    fn tuple_cannot_be_expression_statement() {
        let e = parse_program("BEGIN { (1,2) }").unwrap_err();
        match e {
            crate::error::Error::Parse { msg, .. } => {
                assert!(msg.contains("parenthesized comma list"), "{msg:?}");
            }
            e => panic!("expected parse error, got {e:?}"),
        }
    }

    #[test]
    fn bare_slash_in_expr_is_dollar0_match_not_string() {
        let p = parse_program("BEGIN { r = /foo/ }").unwrap();
        let Stmt::Expr(ex) = first_begin_stmt(&p) else {
            panic!("expected expr stmt");
        };
        let Expr::Assign { rhs, .. } = ex else {
            panic!("expected assign, got {ex:?}");
        };
        match rhs.as_ref() {
            Expr::Binary { op, left, right } => {
                assert_eq!(*op, BinOp::Match);
                assert!(matches!(
                    left.as_ref(),
                    Expr::Field(f) if matches!(f.as_ref(), Expr::Number(n) if *n == 0.0)
                ));
                assert!(matches!(right.as_ref(), Expr::Str(s) if s == "foo"));
            }
            e => panic!("expected $0 ~ /foo/, got {e:?}"),
        }
        let listing = crate::ast_fmt::format_program(&p);
        assert!(
            listing.contains('~') && listing.contains("$0"),
            "pretty-print should show $0 ~ …, got:\n{listing}"
        );
    }

    #[test]
    fn sprintf_bignum_add_parses_integer_literals_not_f64_rounding() {
        let p = parse_program(r#"BEGIN { print sprintf("%d", 9223372036854775807 + 1) }"#).unwrap();
        let stmt = first_begin_stmt(&p);
        let Stmt::Print { args, .. } = stmt else {
            panic!("expected print");
        };
        let Expr::Call { name, args: cargs } = &args[0] else {
            panic!("expected sprintf call");
        };
        assert_eq!(name, "sprintf");
        let Expr::Binary { op, left, .. } = &cargs[1] else {
            panic!("expected binary +");
        };
        assert_eq!(*op, BinOp::Add);
        assert!(
            matches!(left.as_ref(), Expr::IntegerLiteral(s) if s == "9223372036854775807"),
            "left={left:?}"
        );
    }

    #[test]
    fn parses_switch_numeric_and_regex_case_labels() {
        let p = parse_program(
            "BEGIN { switch (2) { case 1: break; case /foo/: break; default: break } }",
        )
        .unwrap();
        match first_begin_stmt(&p) {
            Stmt::Switch { expr, arms } => {
                assert!(expr_is_int(expr, 2));
                assert_eq!(arms.len(), 3);
                assert!(matches!(
                    &arms[0],
                    SwitchArm::Case {
                        label: SwitchLabel::Expr(e),
                        ..
                    } if expr_is_int(e, 1)
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
            Stmt::GetLine {
                pipe_cmd,
                var,
                redir,
            } => {
                assert!(pipe_cmd.is_none());
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
    fn parses_nested_ternary_precedence() {
        let p = parse_program("BEGIN { x = 1 ? 2 : 3 ? 4 : 5 }").unwrap();
        // Ternary is right-associative in most languages, AWK too.
        // 1 ? 2 : (3 ? 4 : 5)
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::Assign { rhs, .. }) => match rhs.as_ref() {
                Expr::Ternary { cond, then_, else_ } => {
                    assert!(expr_is_int(cond, 1));
                    assert!(expr_is_int(then_, 2));
                    match else_.as_ref() {
                        Expr::Ternary { .. } => {}
                        _ => panic!("expected nested ternary on else branch"),
                    }
                }
                _ => panic!("expected ternary"),
            },
            _ => panic!("expected assign"),
        }
    }

    #[test]
    fn parses_for_c_missing_components() {
        // for (;;)
        parse_program("BEGIN { for (;;) break }").unwrap();
        // for (i=0; ; i++)
        parse_program("BEGIN { for (i=0; ; i++) break }").unwrap();
        // for (; i<10; )
        parse_program("BEGIN { for (; i<10; ) break }").unwrap();
    }

    #[test]
    fn parses_multiple_begin_end_blocks() {
        let p =
            parse_program("BEGIN { x=1 } BEGIN { y=2 } END { print x } END { print y }").unwrap();
        assert_eq!(
            p.rules
                .iter()
                .filter(|r| matches!(r.pattern, Pattern::Begin))
                .count(),
            2
        );
        assert_eq!(
            p.rules
                .iter()
                .filter(|r| matches!(r.pattern, Pattern::End))
                .count(),
            2
        );
    }

    #[test]
    fn parses_beginfile_endfile_rules() {
        let p = parse_program("BEGINFILE { print FILENAME } ENDFILE { print NR }").unwrap();
        assert_eq!(
            p.rules
                .iter()
                .filter(|r| matches!(r.pattern, Pattern::BeginFile))
                .count(),
            1
        );
        assert_eq!(
            p.rules
                .iter()
                .filter(|r| matches!(r.pattern, Pattern::EndFile))
                .count(),
            1
        );
    }

    #[test]
    fn parses_switch_nested() {
        let src = r#"BEGIN {
            switch(x) {
                case 1:
                    switch(y) {
                        case 2: print 3; break
                        default: print 4
                    }
                    break
                default:
                    print 5
            }
        }"#;
        parse_program(src).unwrap();
    }

    #[test]
    fn parses_deeply_nested_ternary() {
        let src = "BEGIN { x = a?b:c?d:e?f:g?h:i?j:k }";
        parse_program(src).unwrap();
    }

    #[test]
    fn parses_switch_multiple_labels_per_arm() {
        // gawk allows: case 1: case 2: case 3: print "hit"; break
        let src = "BEGIN { switch(x) { case 1: case 2: case 3: print \"hit\"; break } }";
        parse_program(src).unwrap();
    }

    #[test]
    fn parses_array_delete_multiple_indices() {
        let src = "BEGIN { delete a[1,2,3] }";
        parse_program(src).unwrap();
    }

    #[test]
    fn parses_regexp_constant_gawk() {
        let p = parse_program("BEGIN { x = @/foo/ }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::Assign { rhs, .. }) => {
                assert!(matches!(rhs.as_ref(), Expr::RegexpLiteral(s) if s == "foo"));
            }
            _ => panic!("expected assign of regexp literal"),
        }
    }

    #[test]
    fn parses_field_increment_precedence() {
        // $1++ should be ($1)++
        let p = parse_program("BEGIN { $1++ }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::IncDec { op, target }) => {
                assert_eq!(*op, IncDecOp::PostInc);
                assert!(matches!(target, IncDecTarget::Field(_)));
            }
            _ => panic!("expected increment"),
        }
    }

    #[test]
    fn parses_dollar_neg_one() {
        // $-1 should be $(-1)
        let p = parse_program("BEGIN { print $-1 }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { args, .. } => match &args[0] {
                Expr::Field(e) => match e.as_ref() {
                    Expr::Unary {
                        op: UnaryOp::Neg, ..
                    } => {}
                    _ => panic!("expected negative field index"),
                },
                _ => panic!("expected field"),
            },
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn parses_getline_from_pipe() {
        let p = parse_program("BEGIN { \"cat\" | getline x }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::GetLine {
                pipe_cmd,
                var,
                redir,
            }) => {
                assert!(pipe_cmd.is_some());
                assert_eq!(var.as_deref(), Some("x"));
                assert!(matches!(redir, GetlineRedir::Primary));
            }
            _ => panic!("expected getline"),
        }
    }

    #[test]
    fn parses_delete_whole_array() {
        let p = parse_program("BEGIN { delete a }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { name, indices } => {
                assert_eq!(name, "a");
                assert!(indices.is_none());
            }
            _ => panic!("expected delete"),
        }
    }

    #[test]
    fn parses_delete_array_element() {
        let p = parse_program("BEGIN { delete a[1,2] }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { name, indices } => {
                assert_eq!(name, "a");
                assert_eq!(indices.as_ref().unwrap().len(), 2);
            }
            _ => panic!("expected delete element"),
        }
    }

    #[test]
    fn parses_range_pattern_with_expressions() {
        let p = parse_program("NR==1, NR==10 { print }").unwrap();
        let rule = &p.rules[0];
        match &rule.pattern {
            Pattern::Range(l, r) => match (l.as_ref(), r.as_ref()) {
                (Pattern::Expr(_), Pattern::Expr(_)) => {}
                _ => panic!("expected expr patterns in range"),
            },
            _ => panic!("expected range pattern"),
        }
    }

    #[test]
    fn parses_empty_action_record_rule() {
        let p = parse_program("1").unwrap();
        assert_eq!(p.rules.len(), 1);
        assert!(matches!(p.rules[0].stmts[0], Stmt::Print { .. }));
    }

    #[test]
    fn parses_function_with_no_params() {
        let p = parse_program("function f() { return 1 }").unwrap();
        assert!(p.funcs.contains_key("f"));
        assert_eq!(p.funcs["f"].params.len(), 0);
    }

    #[test]
    fn parses_function_with_multiple_params() {
        let p = parse_program("function f(a, b, c) { return a+b+c }").unwrap();
        assert_eq!(p.funcs["f"].params.len(), 3);
    }

    #[test]
    fn parses_in_operator_compared() {
        parse_program("BEGIN { x = (\"a\" in a) == 0 }").unwrap();
    }

    #[test]
    fn parses_multidimensional_in() {
        parse_program("BEGIN { if ((1,2,3) in a) print }").unwrap();
    }

    #[test]
    fn parses_multidimensional_assign() {
        let src = "BEGIN { a[1,2,3] = 42 }";
        let p = parse_program(src).unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::AssignIndex { ref name, .. }) => {
                assert_eq!(name, "a");
            }
            _ => panic!("expected assign index"),
        }
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
    fn parses_parenthesized_comma_list_in_multidim_in() {
        let p = parse_program("BEGIN { print ((1, 2) in a) }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .unwrap();
        match rule.stmts.first() {
            Some(Stmt::Print { args, .. }) => match &args[0] {
                Expr::In { key, arr } => {
                    assert_eq!(arr, "a");
                    match key.as_ref() {
                        Expr::Tuple(parts) => {
                            assert_eq!(parts.len(), 2);
                            assert!(expr_is_int(&parts[0], 1));
                            assert!(expr_is_int(&parts[1], 2));
                        }
                        _ => panic!("expected tuple key"),
                    }
                }
                _ => panic!("expected `in`"),
            },
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn begin_end_special_patterns_require_braced_action() {
        for src in ["BEGIN", "END", "BEGINFILE", "ENDFILE"] {
            let e = parse_program(src).unwrap_err();
            match e {
                crate::error::Error::Parse { msg, .. } => {
                    assert!(
                        msg.contains("BEGIN") || msg.contains("END"),
                        "src={src:?} msg={msg:?}"
                    );
                }
                e => panic!("unexpected err for {src:?}: {e:?}"),
            }
        }
    }

    #[test]
    fn record_pattern_without_brace_is_default_print_dollar0() {
        let p = parse_program("/x/").unwrap();
        assert_eq!(p.rules.len(), 1);
        match &p.rules[0] {
            Rule {
                pattern: Pattern::Regexp(re),
                stmts,
            } => {
                assert_eq!(re, "x");
                assert!(
                    matches!(
                        stmts.as_slice(),
                        [Stmt::Print { args, redir: None }] if args.is_empty()
                    ),
                    "stmts={stmts:?}"
                );
            }
            r => panic!("unexpected rule: {r:?}"),
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
    fn parses_function_bare_return_statement() {
        let p = parse_program("function f(){ return } BEGIN { }").unwrap();
        let f = p.funcs.get("f").expect("f");
        assert_eq!(f.body.len(), 1);
        assert!(matches!(f.body[0], Stmt::Return(None)));
    }

    #[test]
    fn parses_regexp_literal_at_slash() {
        let p = parse_program("BEGIN { x = @/foo/ }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::Assign { rhs, .. }) => match rhs.as_ref() {
                Expr::RegexpLiteral(s) => assert_eq!(s, "foo"),
                e => panic!("expected RegexpLiteral, got {e:?}"),
            },
            s => panic!("expected assign: {s:?}"),
        }
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

    #[test]
    fn parses_prefix_increment_var() {
        let p = parse_program("BEGIN { ++x }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::IncDec { op, target }) => {
                assert_eq!(*op, IncDecOp::PreInc);
                assert!(matches!(target, IncDecTarget::Var(ref s) if s == "x"));
            }
            s => panic!("expected ++x expr, got {s:?}"),
        }
    }

    #[test]
    fn parses_postfix_increment_field() {
        let p = parse_program("BEGIN { $1++ }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::IncDec { op, target }) => {
                assert_eq!(*op, IncDecOp::PostInc);
                match target {
                    IncDecTarget::Field(inner) => {
                        assert!(expr_is_int(inner.as_ref(), 1));
                    }
                    t => panic!("expected $1++, target={t:?}"),
                }
            }
            s => panic!("expected $1++, got {s:?}"),
        }
    }

    #[test]
    fn parses_prefix_decrement_var() {
        let p = parse_program("BEGIN { --y }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::IncDec { op, target }) => {
                assert_eq!(*op, IncDecOp::PreDec);
                assert!(matches!(target, IncDecTarget::Var(ref s) if s == "y"));
            }
            s => panic!("expected --y, got {s:?}"),
        }
    }

    #[test]
    fn parses_postfix_decrement_array_subscript() {
        let p = parse_program("BEGIN { a[1]-- }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::IncDec { op, target }) => {
                assert_eq!(*op, IncDecOp::PostDec);
                match target {
                    IncDecTarget::Index { name, indices } => {
                        assert_eq!(name, "a");
                        assert_eq!(indices.len(), 1);
                        assert!(expr_is_int(&indices[0], 1));
                    }
                    t => panic!("expected a[1]--, got {t:?}"),
                }
            }
            s => panic!("expected a[1]--, got {s:?}"),
        }
    }

    #[test]
    fn parses_do_while_loop() {
        let p = parse_program("BEGIN { do { print 1 } while (0) }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::DoWhile { body, cond } => {
                assert!(expr_is_int(cond, 0));
                assert_eq!(body.len(), 1);
            }
            s => panic!("expected do-while, got {s:?}"),
        }
    }

    #[test]
    fn parses_for_c_infinite_form_with_break() {
        let p = parse_program("BEGIN { for (;;) break }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::ForC {
                init,
                cond,
                iter,
                body,
            } => {
                assert!(init.is_none() && cond.is_none() && iter.is_none());
                assert_eq!(body.len(), 1);
            }
            s => panic!("expected for (;;), got {s:?}"),
        }
    }

    #[test]
    fn parses_for_c_with_init_cond_iter() {
        let p = parse_program("BEGIN { for (i = 0; i < 3; i++) { print i } }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::ForC {
                init: Some(_),
                cond: Some(_),
                iter: Some(_),
                body,
            } => assert_eq!(body.len(), 1),
            s => panic!("expected for-C, got {s:?}"),
        }
    }

    #[test]
    fn parses_delete_entire_array() {
        let p = parse_program("BEGIN { delete a }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { name, indices } => {
                assert_eq!(name, "a");
                assert!(indices.is_none());
            }
            s => panic!("expected delete a, got {s:?}"),
        }
    }

    #[test]
    fn parses_for_in_loop() {
        let p = parse_program("BEGIN { for (k in arr) print k }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::ForIn { var, arr, body } => {
                assert_eq!(var, "k");
                assert_eq!(arr, "arr");
                assert_eq!(body.len(), 1);
            }
            s => panic!("expected for-in, got {s:?}"),
        }
    }

    #[test]
    fn parses_delete_array_element_multidimensional() {
        let p = parse_program("BEGIN { delete a[1,2] }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { name, indices } => {
                assert_eq!(name, "a");
                let ix = indices.as_ref().expect("indexed delete");
                assert_eq!(ix.len(), 2);
                assert!(expr_is_int(&ix[0], 1));
                assert!(expr_is_int(&ix[1], 2));
            }
            s => panic!("expected delete a[1,2], got {s:?}"),
        }
    }

    #[test]
    fn parses_assign_multidimensional_subscript() {
        let p = parse_program("BEGIN { a[1,2] = 9 }").unwrap();
        let Stmt::Expr(Expr::AssignIndex { name, indices, .. }) = first_begin_stmt(&p) else {
            panic!("expected AssignIndex");
        };
        assert_eq!(name, "a");
        assert_eq!(indices.len(), 2);
        assert!(expr_is_int(&indices[0], 1));
        assert!(expr_is_int(&indices[1], 2));
    }

    #[test]
    fn parses_beginfile_rule() {
        let p = parse_program("BEGINFILE { bf = 1 }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::BeginFile))
            .expect("BEGINFILE rule");
        assert_eq!(rule.stmts.len(), 1);
    }

    #[test]
    fn parses_nextfile_statement() {
        let p = parse_program("BEGIN { nextfile }").unwrap();
        assert!(matches!(first_begin_stmt(&p), Stmt::NextFile));
    }

    #[test]
    fn parses_bare_length_as_zero_arg_call() {
        let p = parse_program("BEGIN { x = length }").unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        assert!(matches!(
            **rhs,
            Expr::Call { ref name, ref args } if name == "length" && args.is_empty()
        ));
    }

    #[test]
    fn parses_endfile_rule() {
        let p = parse_program("ENDFILE { ef = 1 }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::EndFile))
            .expect("ENDFILE rule");
        assert_eq!(rule.stmts.len(), 1);
    }

    #[test]
    fn parses_ternary_expression() {
        let p = parse_program("BEGIN { x = 1 ? 2 : 3 }").unwrap();
        let rule = p
            .rules
            .iter()
            .find(|r| matches!(r.pattern, Pattern::Begin))
            .unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = &rule.stmts[0] else {
            panic!("expected assign");
        };
        assert!(matches!(**rhs, Expr::Ternary { .. }));
    }

    #[test]
    fn parses_postfix_increment_field_in_record_rule() {
        let p = parse_program("{ $1++ }").unwrap();
        assert!(matches!(p.rules[0].pattern, Pattern::Empty));
        match &p.rules[0].stmts[0] {
            Stmt::Expr(Expr::IncDec { op, target }) => {
                assert_eq!(*op, IncDecOp::PostInc);
                assert!(matches!(
                    target,
                    IncDecTarget::Field(inner)
                        if matches!(**inner, Expr::IntegerLiteral(ref s) if s == "1")
                ));
            }
            s => panic!("expected $1++ in record rule, got {s:?}"),
        }
    }

    #[test]
    fn parses_ternary_precedence_v2() {
        // x = a ? b : c ? d : e  => x = a ? b : (c ? d : e)
        let p = parse_program("BEGIN { x = a ? b : c ? d : e }").unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        match rhs.as_ref() {
            Expr::Ternary { else_, .. } => {
                assert!(matches!(else_.as_ref(), Expr::Ternary { .. }));
            }
            _ => panic!("expected ternary, got {rhs:?}"),
        }
    }

    #[test]
    fn parses_in_operator_precedence_v2() {
        // x = a + b in arr => x = (a + b) in arr
        let p = parse_program("BEGIN { x = a + b in arr }").unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        match rhs.as_ref() {
            Expr::In { key, .. } => {
                assert!(matches!(key.as_ref(), Expr::Binary { op: BinOp::Add, .. }));
            }
            _ => panic!("expected In, got {rhs:?}"),
        }
    }

    #[test]
    fn parses_print_redirection_v2() {
        let p = parse_program("BEGIN { print \"hi\" > \"out.txt\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { args, redir, .. } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(redir, Some(PrintRedir::Overwrite(_))));
            }
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn parses_nested_if_else_v2() {
        let p = parse_program("BEGIN { if (1) if (2) a; else b }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::If { then_, else_, .. } => {
                assert!(else_.is_empty()); // The 'else' should belong to 'if (2)'
                match &then_[0] {
                    Stmt::If {
                        else_: inner_else, ..
                    } => {
                        assert!(!inner_else.is_empty());
                    }
                    _ => panic!("expected inner if"),
                }
            }
            _ => panic!("expected outer if"),
        }
    }

    #[test]
    fn parses_nested_while_v2() {
        let p = parse_program("BEGIN { while(1) while(2) print 1 }").unwrap();
        let Stmt::While { body, .. } = first_begin_stmt(&p) else {
            panic!("expected while");
        };
        assert!(matches!(body[0], Stmt::While { .. }));
    }

    #[test]
    fn parses_for_c_empty_v2() {
        let p = parse_program("BEGIN { for(;;) break }").unwrap();
        let Stmt::ForC {
            init, cond, iter, ..
        } = first_begin_stmt(&p)
        else {
            panic!("expected for");
        };
        assert!(init.is_none());
        assert!(cond.is_none());
        assert!(iter.is_none());
    }

    #[test]
    fn parses_function_params_v2() {
        let p = parse_program("function f(a, b, c) { return a+b+c }").unwrap();
        let f = p.funcs.get("f").unwrap();
        assert_eq!(f.params.len(), 3);
    }

    #[test]
    fn parses_function_with_one_param_v2() {
        let p = parse_program("function f(x) { return x }").unwrap();
        assert_eq!(p.funcs.get("f").unwrap().params.len(), 1);
    }

    #[test]
    fn parses_do_while_loop_v2() {
        let p = parse_program("BEGIN { do { print 1 } while (1) }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::DoWhile { .. } => {}
            _ => panic!("expected do-while"),
        }
    }

    #[test]
    fn parses_exit_with_code_v2() {
        let p = parse_program("BEGIN { exit 5 }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Exit(Some(e)) => match e {
                Expr::IntegerLiteral(ref s) if s == "5" => {}
                _ => panic!("expected exit 5, got {:?}", e),
            },
            _ => panic!("expected exit"),
        }
    }

    #[test]
    fn parses_return_expr_v2() {
        let p = parse_program("function f() { return 42 }").unwrap();
        let f = p.funcs.get("f").unwrap();
        match &f.body[0] {
            Stmt::Return(Some(e)) => match e {
                Expr::IntegerLiteral(ref s) if s == "42" => {}
                _ => panic!("expected return 42, got {:?}", e),
            },
            _ => panic!("expected return"),
        }
    }

    #[test]
    fn parses_unary_plus_v2() {
        let p = parse_program("BEGIN { x = +1 }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::Assign { rhs, .. }) => {
                assert!(matches!(
                    rhs.as_ref(),
                    Expr::Unary {
                        op: UnaryOp::Pos,
                        ..
                    }
                ));
            }
            _ => panic!("expected assign"),
        }
    }

    #[test]
    fn parses_grouping_parens_v2() {
        let p = parse_program("BEGIN { x = (1+2)*3 }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::Assign { rhs, .. }) => {
                assert!(matches!(rhs.as_ref(), Expr::Binary { op: BinOp::Mul, .. }));
            }
            _ => panic!("expected assign"),
        }
    }

    #[test]
    fn parses_multidim_index_v2() {
        let p = parse_program("BEGIN { print a[1, 2, 3] }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { args, .. } => match &args[0] {
                Expr::Index { indices, .. } => assert_eq!(indices.len(), 3),
                _ => panic!("expected index"),
            },
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn parses_if_else_if_v2() {
        let p = parse_program("BEGIN { if(1) a; else if(2) b; else c }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::If { else_, .. } => {
                assert!(!else_.is_empty());
                assert!(matches!(else_[0], Stmt::If { .. }));
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn parses_for_in_loop_v3() {
        let p = parse_program("BEGIN { for (k in a) print k }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::ForIn { .. } => {}
            _ => panic!("expected for-in"),
        }
    }

    #[test]
    fn parses_delete_whole_array_v2() {
        let p = parse_program("BEGIN { delete a }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { indices, .. } => assert!(indices.is_none()),
            _ => panic!("expected delete"),
        }
    }

    #[test]
    fn parses_delete_element_v2() {
        let p = parse_program("BEGIN { delete a[1] }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Delete { indices, .. } => assert!(indices.is_some()),
            _ => panic!("expected delete"),
        }
    }

    #[test]
    fn parses_indirect_call_at_literal_v2() {
        // gawk: @("func")(1) is the form for literal string as function name
        let p = parse_program("BEGIN { @(\"func\")(1) }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::IndirectCall { .. }) => {}
            _ => panic!("expected indirect call"),
        }
    }

    #[test]
    fn parses_getline_v3() {
        let p = parse_program("BEGIN { getline }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::GetLine { .. } => {}
            _ => panic!("expected getline"),
        }
    }

    #[test]
    fn parses_getline_into_var_v2() {
        let p = parse_program("BEGIN { getline x }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::GetLine { var, .. } => assert!(var.is_some()),
            _ => panic!("expected getline"),
        }
    }

    #[test]
    fn parses_next_v2() {
        let p = parse_program("{ next }").unwrap();
        match &p.rules[0].stmts[0] {
            Stmt::Next => {}
            _ => panic!("expected next"),
        }
    }

    #[test]
    fn parses_nextfile_v2() {
        let p = parse_program("{ nextfile }").unwrap();
        match &p.rules[0].stmts[0] {
            Stmt::NextFile => {}
            _ => panic!("expected nextfile"),
        }
    }

    #[test]
    fn parses_printf_v2() {
        let p = parse_program("BEGIN { printf \"%d\", 1 }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Printf { .. } => {}
            _ => panic!("expected printf"),
        }
    }

    #[test]
    fn parses_regex_match_v2() {
        let p = parse_program("BEGIN { if ($0 ~ /foo/) print 1 }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::If { cond, .. } => {
                assert!(matches!(
                    cond,
                    Expr::Binary {
                        op: BinOp::Match,
                        ..
                    }
                ));
            }
            _ => panic!("expected if"),
        }
    }

    #[test]
    fn parses_concatenation_v2() {
        let p = parse_program("BEGIN { print \"a\" \"b\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { args, .. } => {
                assert!(matches!(
                    args[0],
                    Expr::Binary {
                        op: BinOp::Concat,
                        ..
                    }
                ));
            }
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn parses_empty_block_v2() {
        let p = parse_program("BEGIN { {} }").unwrap();
        assert!(p.rules.len() == 1);
    }

    #[test]
    fn parses_print_pipe_v2() {
        let p = parse_program("BEGIN { print 1 | \"cmd\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { redir, .. } => {
                assert!(matches!(redir, Some(PrintRedir::Pipe(_))));
            }
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn parses_print_append_v2() {
        let p = parse_program("BEGIN { print 1 >> \"out.txt\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Print { redir, .. } => {
                assert!(matches!(redir, Some(PrintRedir::Append(_))));
            }
            _ => panic!("expected print"),
        }
    }

    #[test]
    fn parses_bitwise_precedence_v2() {
        let p = parse_program("BEGIN { x = and(a, b) + or(c, d) }").unwrap();
        assert!(p.rules.len() == 1);
    }

    #[test]
    fn parses_ternary_nested_precedence_v3() {
        let p = parse_program("BEGIN { x = a ? b : c ? d : e }").unwrap();
        let Stmt::Expr(Expr::Assign { rhs, .. }) = first_begin_stmt(&p) else {
            panic!("expected assign");
        };
        match rhs.as_ref() {
            Expr::Ternary { else_, .. } => {
                assert!(matches!(else_.as_ref(), Expr::Ternary { .. }));
            }
            _ => panic!("expected ternary"),
        }
    }

    #[test]
    fn parses_logical_precedence_v2() {
        // a || b && c => a || (b && c)
        let p = parse_program("BEGIN { if (a || b && c) print 1 }").unwrap();
        let Stmt::If { cond, .. } = first_begin_stmt(&p) else {
            panic!("expected if");
        };
        match cond {
            Expr::Binary {
                op: BinOp::Or,
                right,
                ..
            } => {
                assert!(matches!(
                    right.as_ref(),
                    Expr::Binary { op: BinOp::And, .. }
                ));
            }
            _ => panic!("expected Or, got {cond:?}"),
        }
    }

    #[test]
    fn parses_getline_file_v6() {
        let p = parse_program("BEGIN { getline < \"f\" }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::GetLine { redir, .. } => assert!(matches!(redir, GetlineRedir::File(_))),
            _ => panic!("expected getline"),
        }
    }

    #[test]
    fn parses_getline_pipe_v6() {
        let p = parse_program("BEGIN { \"c\" | getline }").unwrap();
        match first_begin_stmt(&p) {
            Stmt::Expr(Expr::GetLine { pipe_cmd, .. }) => assert!(pipe_cmd.is_some()),
            _ => panic!("expected getline"),
        }
    }

    #[test]
    fn parses_function_no_params_v6() {
        let p = parse_program("function f() { }").unwrap();
        assert_eq!(p.funcs.get("f").unwrap().params.len(), 0);
    }
}

// ── Parser pinning: operator precedence, error messages, edge syntax ─────────
//
// Operator precedence regressions are notoriously easy to introduce when
// extending the grammar — `a + b * c` flipping to `(a+b)*c` would silently
// break user math. These tests pin the precedence ordering by inspecting the
// AST shape, not just the parse-doesn't-error outcome. Same for syntax errors:
// pin that they happen, so a regression that silently accepts invalid syntax
// surfaces as a named test failure.

#[cfg(test)]
mod parser_pinning {
    use super::parse_program;
    use crate::ast::{BinOp, Expr, Pattern, Stmt};

    fn first_begin_expr_stmt(src: &str) -> Expr {
        let prog = parse_program(src).expect("parse");
        let stmt = prog
            .rules
            .iter()
            .find_map(|r| {
                if matches!(r.pattern, Pattern::Begin) {
                    r.stmts.first()
                } else {
                    None
                }
            })
            .expect("BEGIN body");
        match stmt {
            Stmt::Expr(e) => e.clone(),
            Stmt::Print { args, .. } => args[0].clone(),
            other => panic!("first BEGIN stmt is not expr/print: {other:?}"),
        }
    }

    // ── Precedence: arithmetic ───────────────────────────────────────────────

    #[test]
    fn mul_binds_tighter_than_add() {
        // `a + b * c` must parse as `a + (b * c)`, not `(a + b) * c`.
        let e = first_begin_expr_stmt("BEGIN { x = a + b * c }");
        if let Expr::Assign { rhs, .. } = e {
            match *rhs {
                Expr::Binary {
                    op: BinOp::Add,
                    left,
                    right,
                } => {
                    assert!(matches!(*left, Expr::Var(_)), "left should be plain a");
                    assert!(
                        matches!(*right, Expr::Binary { op: BinOp::Mul, .. }),
                        "right should be (b * c), got {right:?}"
                    );
                }
                other => panic!("expected Add root, got {other:?}"),
            }
        } else {
            panic!("not an assign expr: {e:?}");
        }
    }

    #[test]
    fn pow_is_right_associative() {
        // POSIX awk: `2 ^ 3 ^ 2` = `2 ^ (3 ^ 2)` = 512, not `(2^3)^2` = 64.
        let e = first_begin_expr_stmt("BEGIN { x = 2 ^ 3 ^ 2 }");
        if let Expr::Assign { rhs, .. } = e {
            match *rhs {
                Expr::Binary {
                    op: BinOp::Pow,
                    right,
                    ..
                } => {
                    // Right side must itself be a Pow expression.
                    assert!(
                        matches!(*right, Expr::Binary { op: BinOp::Pow, .. }),
                        "pow must be right-assoc, got {right:?}"
                    );
                }
                other => panic!("expected Pow root, got {other:?}"),
            }
        }
    }

    #[test]
    fn unary_minus_binds_outside_pow() {
        // POSIX: `-2 ^ 2` = `-(2^2)` = -4, not `(-2)^2` = 4.
        let e = first_begin_expr_stmt("BEGIN { x = -2 ^ 2 }");
        if let Expr::Assign { rhs, .. } = e {
            // Should be Unary(Neg, Binary(Pow, 2, 2))
            assert!(
                matches!(*rhs, Expr::Unary { .. }),
                "unary should wrap pow, got {rhs:?}"
            );
        }
    }

    #[test]
    fn comparison_lower_than_arithmetic() {
        // `a < b + c` parses as `a < (b + c)`.
        let e = first_begin_expr_stmt("BEGIN { x = a < b + c }");
        if let Expr::Assign { rhs, .. } = e {
            match *rhs {
                Expr::Binary {
                    op: BinOp::Lt,
                    right,
                    ..
                } => {
                    assert!(
                        matches!(*right, Expr::Binary { op: BinOp::Add, .. }),
                        "comparison should embed the add, got {right:?}"
                    );
                }
                other => panic!("expected Lt root, got {other:?}"),
            }
        }
    }

    #[test]
    fn logical_and_lower_than_comparison() {
        // `a < b && c > d` → `(a<b) && (c>d)`
        let e = first_begin_expr_stmt("BEGIN { x = (a < b && c > d) }");
        if let Expr::Assign { rhs, .. } = e {
            // The top operator should be And.
            match *rhs {
                Expr::Binary {
                    op: BinOp::And,
                    left,
                    right,
                } => {
                    assert!(matches!(*left, Expr::Binary { op: BinOp::Lt, .. }));
                    assert!(matches!(*right, Expr::Binary { op: BinOp::Gt, .. }));
                }
                other => panic!("expected And root, got {other:?}"),
            }
        }
    }

    #[test]
    fn logical_or_lower_than_and() {
        // `a || b && c` → `a || (b && c)`
        let e = first_begin_expr_stmt("BEGIN { x = a || b && c }");
        if let Expr::Assign { rhs, .. } = e {
            match *rhs {
                Expr::Binary {
                    op: BinOp::Or,
                    right,
                    ..
                } => {
                    assert!(
                        matches!(*right, Expr::Binary { op: BinOp::And, .. }),
                        "Or-right should be And, got {right:?}"
                    );
                }
                other => panic!("expected Or root, got {other:?}"),
            }
        }
    }

    #[test]
    fn ternary_below_logical_or() {
        // `a || b ? c : d` parses as `(a || b) ? c : d`
        let e = first_begin_expr_stmt("BEGIN { x = a || b ? c : d }");
        if let Expr::Assign { rhs, .. } = e {
            match *rhs {
                Expr::Ternary { cond, .. } => {
                    assert!(
                        matches!(*cond, Expr::Binary { op: BinOp::Or, .. }),
                        "ternary cond should be Or expr, got {cond:?}"
                    );
                }
                other => panic!("expected Ternary, got {other:?}"),
            }
        }
    }

    // ── Concat: implicit, just below comparison ──────────────────────────────

    #[test]
    fn concat_lower_than_arithmetic_higher_than_comparison() {
        // `"a" b + c` → `"a" (b + c)` (concat of "a" with the sum)
        let e = first_begin_expr_stmt("BEGIN { x = \"a\" b + c }");
        if let Expr::Assign { rhs, .. } = e {
            match *rhs {
                Expr::Binary {
                    op: BinOp::Concat,
                    right,
                    ..
                } => {
                    assert!(
                        matches!(*right, Expr::Binary { op: BinOp::Add, .. }),
                        "concat right should be the add, got {right:?}"
                    );
                }
                other => panic!("expected Concat root, got {other:?}"),
            }
        }
    }

    // ── Syntax errors ────────────────────────────────────────────────────────

    #[test]
    fn unclosed_brace_is_parse_error() {
        assert!(parse_program("BEGIN { ").is_err());
    }

    #[test]
    fn unclosed_paren_in_expression_is_parse_error() {
        assert!(parse_program("BEGIN { x = (1 + 2 }").is_err());
    }

    #[test]
    fn unclosed_string_is_parse_error() {
        assert!(parse_program("BEGIN { x = \"abc }").is_err());
    }

    #[test]
    fn invalid_assignment_target_rejected() {
        // `1 = 2` is not valid — LHS must be lvalue.
        assert!(parse_program("BEGIN { 1 = 2 }").is_err());
    }

    #[test]
    fn duplicate_function_name_rejected() {
        let r = parse_program("function f() { } function f() { } BEGIN { }");
        assert!(r.is_err(), "duplicate function defs must error");
    }

    #[test]
    fn return_outside_function_parses_but_run_errors() {
        // `return` in rule body parses (some awks allow); awkrs's VM rejects it
        // at runtime. Pin the parse-OK side here.
        let r = parse_program("BEGIN { return 1 }");
        assert!(r.is_ok(), "return parses; runtime rejects");
    }

    #[test]
    fn parenthesized_comma_list_alone_rejected_as_statement() {
        // `(1, 2)` as a bare statement (no print/printf context) is invalid.
        assert!(parse_program("BEGIN { (1, 2) }").is_err());
    }

    // ── Edge syntax: regexp constants ────────────────────────────────────────

    #[test]
    fn slash_after_keyword_is_regex_not_division() {
        // After `if`/`while`/`for`/`{`, `/foo/` is a regex literal.
        let prog = parse_program("BEGIN { if (1) /foo/ }").expect("parse");
        // Just verify it doesn't error — the AST shape is in other tests.
        assert!(!prog.rules.is_empty());
    }

    #[test]
    fn typed_regex_at_slash_parses() {
        let prog = parse_program(r#"BEGIN { x = @/abc/ }"#).expect("parse");
        let e = match &prog.rules[0].stmts[0] {
            Stmt::Expr(Expr::Assign { rhs, .. }) => rhs,
            _ => panic!("expected assign"),
        };
        assert!(matches!(**e, Expr::RegexpLiteral(_)));
    }

    // ── Scientific notation literal in source (lexer + parser integration) ───

    #[test]
    fn scientific_literal_parses_as_single_number() {
        // After lexer fix: `1e7`, `2.5e+3`, `1E-2` are single Number tokens
        // and parse as Expr::Number(value), not concat of int and ident.
        for (src, expected) in &[
            ("BEGIN { x = 1e7 }", 1e7),
            ("BEGIN { x = 2.5e+3 }", 2.5e3),
            ("BEGIN { x = 1E-2 }", 1e-2),
        ] {
            let prog = parse_program(src).expect("parse");
            let e = match &prog.rules[0].stmts[0] {
                Stmt::Expr(Expr::Assign { rhs, .. }) => rhs.as_ref().clone(),
                _ => panic!("expected assign"),
            };
            match e {
                Expr::Number(n) => assert!(
                    (n - expected).abs() / expected.abs().max(1.0) < 1e-12,
                    "{src}: got {n}, expected {expected}"
                ),
                other => panic!("{src}: expected Number, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_basic_if_else() {
        let p = parse_program("BEGIN { if (1) print 1; else print 0 }").unwrap();
        assert_eq!(p.rules.len(), 1);
    }

    #[test]
    fn parse_while_loop() {
        let p = parse_program("BEGIN { while (1) print 1 }").unwrap();
        assert_eq!(p.rules.len(), 1);
    }

    #[test]
    fn parse_for_c_loop() {
        let p = parse_program("BEGIN { for (i=0; i<10; i++) print i }").unwrap();
        assert_eq!(p.rules.len(), 1);
    }

    #[test]
    fn parse_for_in_loop() {
        let p = parse_program("BEGIN { for (x in a) print x }").unwrap();
        assert_eq!(p.rules.len(), 1);
    }

    #[test]
    fn parse_delete_statement() {
        let p = parse_program("BEGIN { delete a[1]; delete b }").unwrap();
        assert_eq!(p.rules.len(), 1);
    }

    #[test]
    fn parse_error_unexpected_token() {
        let r = parse_program("BEGIN { print + }");
        assert!(r.is_err());
    }

    #[test]
    fn parse_error_missing_rbracket() {
        let r = parse_program("BEGIN { a[1 = 2 }");
        assert!(r.is_err());
    }

    #[test]
    fn parse_error_duplicate_function() {
        let r = parse_program("function f(){} function f(){}");
        assert!(r.is_err());
    }

    #[test]
    fn parse_error_malformed_for_in() {
        let r = parse_program("BEGIN { for (x a) print x }");
        assert!(r.is_err());
    }

    #[test]
    fn parses_empty_function_v2() {
        let p = parse_program("function f() {}").unwrap();
        assert!(p.funcs.contains_key("f"));
        assert!(p.funcs.get("f").unwrap().body.is_empty());
    }

    #[test]
    fn parse_error_at_directive_outside_top_level_v2() {
        // gawk: @include etc are only at top level.
        // Our parser actually doesn't see them because expander strips them,
        // but if they remained, it would be a parse error.
        let r = parse_program("BEGIN { @include \"foo\" }");
        assert!(r.is_err());
    }

    #[test]
    fn parses_num_v13() {
        parse_program("BEGIN { 0 }").unwrap();
    }
    #[test]
    fn parses_num_v13_1() {
        parse_program("BEGIN { 1 }").unwrap();
    }
    #[test]
    fn parses_str_v13() {
        parse_program("BEGIN { \"\" }").unwrap();
    }
    #[test]
    fn parses_str_v13_1() {
        parse_program("BEGIN { \"a\" }").unwrap();
    }
    #[test]
    fn parses_var_v13() {
        parse_program("BEGIN { x }").unwrap();
    }
    #[test]
    fn parses_field_v13() {
        parse_program("BEGIN { $0 }").unwrap();
    }
    #[test]
    fn parses_field_v13_1() {
        parse_program("BEGIN { $1 }").unwrap();
    }
    #[test]
    fn parses_index_v13() {
        parse_program("BEGIN { a[1] }").unwrap();
    }
    #[test]
    fn parses_call_v13() {
        parse_program("BEGIN { f() }").unwrap();
    }
    #[test]
    fn parses_unary_v13() {
        parse_program("BEGIN { !1 }").unwrap();
    }
    #[test]
    fn parses_binary_v13() {
        parse_program("BEGIN { 1+1 }").unwrap();
    }
    #[test]
    fn parses_assign_v13() {
        parse_program("BEGIN { x=1 }").unwrap();
    }
    #[test]
    fn parses_ternary_v13() {
        parse_program("BEGIN { 1?1:0 }").unwrap();
    }
    #[test]
    fn parses_in_v13() {
        parse_program("BEGIN { 1 in a }").unwrap();
    }
    #[test]
    fn parses_if_v13() {
        parse_program("BEGIN { if(1) 1 }").unwrap();
    }
    #[test]
    fn parses_while_v13() {
        parse_program("BEGIN { while(1) 1 }").unwrap();
    }
    #[test]
    fn parses_for_v13() {
        parse_program("BEGIN { for(;;) 1 }").unwrap();
    }
    #[test]
    fn parses_forin_v13() {
        parse_program("BEGIN { for(k in a) 1 }").unwrap();
    }
    #[test]
    fn parses_block_v13() {
        parse_program("BEGIN { {1} }").unwrap();
    }
    #[test]
    fn parses_print_v13() {
        parse_program("BEGIN { print 1 }").unwrap();
    }
    #[test]
    fn parses_printf_v13() {
        parse_program("BEGIN { printf 1 }").unwrap();
    }

    #[test]
    fn parses_break_v17() {
        parse_program("BEGIN { for(;;) break }").unwrap();
    }
    #[test]
    fn parses_continue_v17() {
        parse_program("BEGIN { for(;;) continue }").unwrap();
    }
    #[test]
    fn parses_next_v17() {
        parse_program("{ next }").unwrap();
    }
    #[test]
    fn parses_nextfile_v17() {
        parse_program("{ nextfile }").unwrap();
    }
    #[test]
    fn parses_delete_v17() {
        parse_program("BEGIN { delete a }").unwrap();
    }
    #[test]
    fn parses_exit_v17() {
        parse_program("BEGIN { exit }").unwrap();
    }
    #[test]
    fn parses_return_v17() {
        parse_program("function f(){return}").unwrap();
    }
    #[test]
    fn parses_do_while_v17() {
        parse_program("BEGIN { do 1; while(1) }").unwrap();
    }
    #[test]
    fn parses_regex_match_v17() {
        parse_program("BEGIN { 1 ~ /a/ }").unwrap();
    }
    #[test]
    fn parses_regex_not_match_v17() {
        parse_program("BEGIN { 1 !~ /a/ }").unwrap();
    }
    #[test]
    fn parses_power_v17() {
        parse_program("BEGIN { 1 ** 2 }").unwrap();
    }

    #[test]
    fn parses_add_assign_v20() {
        parse_program("BEGIN { x += 1 }").unwrap();
    }
    #[test]
    fn parses_sub_assign_v20() {
        parse_program("BEGIN { x -= 1 }").unwrap();
    }
    #[test]
    fn parses_mul_assign_v20() {
        parse_program("BEGIN { x *= 1 }").unwrap();
    }
    #[test]
    fn parses_div_assign_v20() {
        parse_program("BEGIN { x /= 1 }").unwrap();
    }
    #[test]
    fn parses_mod_assign_v20() {
        parse_program("BEGIN { x %= 1 }").unwrap();
    }
    #[test]
    fn parses_pow_assign_v20() {
        parse_program("BEGIN { x ^= 1 }").unwrap();
    }
    #[test]
    fn parses_pow_assign_starstar_v20() {
        parse_program("BEGIN { x **= 1 }").unwrap();
    }

    #[test]
    fn parses_pre_inc_v20() {
        parse_program("BEGIN { ++x }").unwrap();
    }
    #[test]
    fn parses_post_inc_v20() {
        parse_program("BEGIN { x++ }").unwrap();
    }
    #[test]
    fn parses_pre_dec_v20() {
        parse_program("BEGIN { --x }").unwrap();
    }
    #[test]
    fn parses_post_dec_v20() {
        parse_program("BEGIN { x-- }").unwrap();
    }

    #[test]
    fn parses_logical_and_v20() {
        parse_program("BEGIN { 1 && 1 }").unwrap();
    }
    #[test]
    fn parses_logical_or_v20() {
        parse_program("BEGIN { 1 || 1 }").unwrap();
    }
    #[test]
    fn parses_logical_not_v20() {
        parse_program("BEGIN { !1 }").unwrap();
    }

    #[test]
    fn parses_cmp_eq_v20() {
        parse_program("BEGIN { 1 == 1 }").unwrap();
    }
    #[test]
    fn parses_cmp_ne_v20() {
        parse_program("BEGIN { 1 != 1 }").unwrap();
    }
    #[test]
    fn parses_cmp_lt_v20() {
        parse_program("BEGIN { 1 < 1 }").unwrap();
    }
    #[test]
    fn parses_cmp_le_v20() {
        parse_program("BEGIN { 1 <= 1 }").unwrap();
    }
    #[test]
    fn parses_cmp_gt_v20() {
        parse_program("BEGIN { 1 > 1 }").unwrap();
    }
    #[test]
    fn parses_cmp_ge_v20() {
        parse_program("BEGIN { 1 >= 1 }").unwrap();
    }

    #[test]
    fn parses_array_delete_elem_v20() {
        parse_program("BEGIN { delete a[1] }").unwrap();
    }
    #[test]
    fn parses_indirect_call_v20() {
        parse_program("BEGIN { @f() }").unwrap();
    }

    #[test]
    fn parses_switch_v32() {
        parse_program("BEGIN { switch(x) { case 1: break; default: break } }").unwrap();
    }
    #[test]
    fn parses_getline_v32() {
        parse_program("BEGIN { getline }").unwrap();
    }
    #[test]
    fn parses_getline_var_v32() {
        parse_program("BEGIN { getline x }").unwrap();
    }
    #[test]
    fn parses_getline_file_v32() {
        parse_program("BEGIN { getline < \"f\" }").unwrap();
    }
    #[test]
    fn parses_getline_pipe_v32() {
        parse_program("BEGIN { \"c\" | getline }").unwrap();
    }

    #[test]
    fn parses_next_v32() {
        parse_program("{ next }").unwrap();
    }
    #[test]
    fn parses_nextfile_v32() {
        parse_program("{ nextfile }").unwrap();
    }
    #[test]
    fn parses_exit_v32() {
        parse_program("BEGIN { exit }").unwrap();
    }
    #[test]
    fn parses_exit_code_v32() {
        parse_program("BEGIN { exit 1 }").unwrap();
    }
    #[test]
    fn parses_return_v32() {
        parse_program("function f() { return }").unwrap();
    }
    #[test]
    fn parses_return_val_v32() {
        parse_program("function f() { return 1 }").unwrap();
    }

    #[test]
    fn parses_array_in_v32() {
        parse_program("BEGIN { 1 in a }").unwrap();
    }
    #[test]
    fn parses_array_delete_v32() {
        parse_program("BEGIN { delete a }").unwrap();
    }
    #[test]
    fn parses_array_delete_elem_v32() {
        parse_program("BEGIN { delete a[1] }").unwrap();
    }

    #[test]
    fn parses_math_v32() {
        parse_program("BEGIN { 1+2-3*4/5%6^7 }").unwrap();
    }
    #[test]
    fn parses_logical_v32() {
        parse_program("BEGIN { 1&&2||!3 }").unwrap();
    }
    #[test]
    fn parses_cmp_v32() {
        parse_program("BEGIN { 1<2&&2<=3||3>4&&4>=5 }").unwrap();
    }
    #[test]
    fn parses_match_v32() {
        parse_program("BEGIN { 1 ~ /a/ && 1 !~ /b/ }").unwrap();
    }
    #[test]
    fn parses_ternary_v32() {
        parse_program("BEGIN { 1?2:3 }").unwrap();
    }

    #[test]
    fn parses_if_v36() {
        parse_program("BEGIN { if(1) 1 }").unwrap();
    }
    #[test]
    fn parses_if_else_v36() {
        parse_program("BEGIN { if(1) 1; else 2 }").unwrap();
    }
    #[test]
    fn parses_while_v36() {
        parse_program("BEGIN { while(1) 1 }").unwrap();
    }
    #[test]
    fn parses_do_while_v36() {
        parse_program("BEGIN { do 1; while(1) }").unwrap();
    }
    #[test]
    fn parses_for_v36() {
        parse_program("BEGIN { for(;;) break }").unwrap();
    }
    #[test]
    fn parses_forin_v36() {
        parse_program("BEGIN { for(k in a) break }").unwrap();
    }

    #[test]
    fn parses_block_v36() {
        parse_program("BEGIN { {1} }").unwrap();
    }
    #[test]
    fn parses_print_v36() {
        parse_program("BEGIN { print 1 }").unwrap();
    }
    #[test]
    fn parses_printf_v36() {
        parse_program("BEGIN { printf 1 }").unwrap();
    }

    #[test]
    fn parses_num_v36() {
        parse_program("BEGIN { 0 }").unwrap();
    }
    #[test]
    fn parses_str_v36() {
        parse_program("BEGIN { \"\" }").unwrap();
    }
    #[test]
    fn parses_var_v36() {
        parse_program("BEGIN { x }").unwrap();
    }
    #[test]
    fn parses_field_v36() {
        parse_program("BEGIN { $0 }").unwrap();
    }
    #[test]
    fn parses_index_v36() {
        parse_program("BEGIN { a[1] }").unwrap();
    }
    #[test]
    fn parses_call_v36() {
        parse_program("BEGIN { f() }").unwrap();
    }
    #[test]
    fn parses_unary_v36() {
        parse_program("BEGIN { !1 }").unwrap();
    }
    #[test]
    fn parses_binary_v36() {
        parse_program("BEGIN { 1+1 }").unwrap();
    }
    #[test]
    fn parses_assign_v36() {
        parse_program("BEGIN { x=1 }").unwrap();
    }
    #[test]
    fn parses_ternary_v36() {
        parse_program("BEGIN { 1?1:0 }").unwrap();
    }
    #[test]
    fn parses_in_v36() {
        parse_program("BEGIN { 1 in a }").unwrap();
    }

    #[test]
    fn parses_exit_v36() {
        parse_program("BEGIN { exit }").unwrap();
    }
    #[test]
    fn parses_next_v36() {
        parse_program("{ next }").unwrap();
    }
    #[test]
    fn parses_nextfile_v36() {
        parse_program("{ nextfile }").unwrap();
    }
    #[test]
    fn parses_delete_v36() {
        parse_program("BEGIN { delete a }").unwrap();
    }
    #[test]
    fn parses_break_v36() {
        parse_program("BEGIN { while(1) break }").unwrap();
    }
    #[test]
    fn parses_continue_v36() {
        parse_program("BEGIN { while(1) continue }").unwrap();
    }
    #[test]
    fn parses_return_v36() {
        parse_program("function f(){return}").unwrap();
    }

    #[test]
    fn parses_stmt_if_v48_0() {
        parse_program("BEGIN{if(1){}}").unwrap();
    }
    #[test]
    fn parses_stmt_if_v48_1() {
        parse_program("BEGIN{if(1){}else{}}").unwrap();
    }
    #[test]
    fn parses_stmt_while_v48() {
        parse_program("BEGIN{while(1){}}").unwrap();
    }
    #[test]
    fn parses_stmt_do_v48() {
        parse_program("BEGIN{do{}while(1)}").unwrap();
    }
    #[test]
    fn parses_stmt_for_v48() {
        parse_program("BEGIN{for(;;){}}").unwrap();
    }
    #[test]
    fn parses_stmt_forin_v48() {
        parse_program("BEGIN{for(k in a){}}").unwrap();
    }
    #[test]
    fn parses_stmt_break_v48() {
        parse_program("BEGIN{while(1)break}").unwrap();
    }
    #[test]
    fn parses_stmt_continue_v48() {
        parse_program("BEGIN{while(1)continue}").unwrap();
    }
    #[test]
    fn parses_stmt_next_v48() {
        parse_program("{next}").unwrap();
    }
    #[test]
    fn parses_stmt_nextfile_v48() {
        parse_program("{nextfile}").unwrap();
    }
    #[test]
    fn parses_stmt_exit_v48() {
        parse_program("BEGIN{exit}").unwrap();
    }
    #[test]
    fn parses_stmt_exit_val_v48() {
        parse_program("BEGIN{exit 1}").unwrap();
    }
    #[test]
    fn parses_stmt_delete_v48() {
        parse_program("BEGIN{delete a[1]}").unwrap();
    }
    #[test]
    fn parses_stmt_delete_all_v48() {
        parse_program("BEGIN{delete a}").unwrap();
    }
    #[test]
    fn parses_stmt_print_v48() {
        parse_program("BEGIN{print}").unwrap();
    }
    #[test]
    fn parses_stmt_print_val_v48() {
        parse_program("BEGIN{print 1}").unwrap();
    }
    #[test]
    fn parses_stmt_printf_v48() {
        parse_program("BEGIN{printf \"\"}").unwrap();
    }
    #[test]
    fn parses_stmt_printf_val_v48() {
        parse_program("BEGIN{printf \"\",1}").unwrap();
    }
    #[test]
    fn parses_stmt_getline_v48() {
        parse_program("BEGIN{getline}").unwrap();
    }
    #[test]
    fn parses_stmt_getline_var_v48() {
        parse_program("BEGIN{getline x}").unwrap();
    }
    #[test]
    fn parses_stmt_getline_file_v48() {
        parse_program("BEGIN{getline <\"f\"}").unwrap();
    }
    #[test]
    fn parses_stmt_getline_pipe_v48() {
        parse_program("BEGIN{\"c\"|getline}").unwrap();
    }

    #[test]
    fn parses_expr_num_v48() {
        parse_program("BEGIN{1}").unwrap();
    }
    #[test]
    fn parses_expr_str_v48() {
        parse_program("BEGIN{\"\"}").unwrap();
    }
    #[test]
    fn parses_expr_var_v48() {
        parse_program("BEGIN{x}").unwrap();
    }
    #[test]
    fn parses_expr_field_v48() {
        parse_program("BEGIN{$1}").unwrap();
    }
    #[test]
    fn parses_expr_index_v48() {
        parse_program("BEGIN{a[1]}").unwrap();
    }
    #[test]
    fn parses_expr_index_multi_v48() {
        parse_program("BEGIN{a[1,2]}").unwrap();
    }
    #[test]
    fn parses_expr_call_v48() {
        parse_program("BEGIN{f()}").unwrap();
    }
    #[test]
    fn parses_expr_call_arg_v48() {
        parse_program("BEGIN{f(1)}").unwrap();
    }
    #[test]
    fn parses_expr_call_args_v48() {
        parse_program("BEGIN{f(1,2)}").unwrap();
    }
    #[test]
    fn parses_expr_unary_v48_0() {
        parse_program("BEGIN{+1}").unwrap();
    }
    #[test]
    fn parses_expr_unary_v48_1() {
        parse_program("BEGIN{-1}").unwrap();
    }
    #[test]
    fn parses_expr_unary_v48_2() {
        parse_program("BEGIN{!1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_0() {
        parse_program("BEGIN{1+1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_1() {
        parse_program("BEGIN{1-1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_2() {
        parse_program("BEGIN{1*1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_3() {
        parse_program("BEGIN{1/1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_4() {
        parse_program("BEGIN{1%1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_5() {
        parse_program("BEGIN{1^1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_6() {
        parse_program("BEGIN{1**1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_7() {
        parse_program("BEGIN{1<1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_8() {
        parse_program("BEGIN{1<=1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_9() {
        parse_program("BEGIN{1>1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_10() {
        parse_program("BEGIN{1>=1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_11() {
        parse_program("BEGIN{1==1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_12() {
        parse_program("BEGIN{1!=1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_13() {
        parse_program("BEGIN{1~\"\"}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_14() {
        parse_program("BEGIN{1!~\"\"}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_15() {
        parse_program("BEGIN{1&&1}").unwrap();
    }
    #[test]
    fn parses_expr_binary_v48_16() {
        parse_program("BEGIN{1||1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_0() {
        parse_program("BEGIN{x=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_1() {
        parse_program("BEGIN{x+=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_2() {
        parse_program("BEGIN{x-=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_3() {
        parse_program("BEGIN{x*=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_4() {
        parse_program("BEGIN{x/=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_5() {
        parse_program("BEGIN{x%=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_6() {
        parse_program("BEGIN{x^=1}").unwrap();
    }
    #[test]
    fn parses_expr_assign_v48_7() {
        parse_program("BEGIN{x**=1}").unwrap();
    }
    #[test]
    fn parses_expr_incdec_v48_0() {
        parse_program("BEGIN{++x}").unwrap();
    }
    #[test]
    fn parses_expr_incdec_v48_1() {
        parse_program("BEGIN{x++}").unwrap();
    }
    #[test]
    fn parses_expr_incdec_v48_2() {
        parse_program("BEGIN{--x}").unwrap();
    }
    #[test]
    fn parses_expr_incdec_v48_3() {
        parse_program("BEGIN{x--}").unwrap();
    }
    #[test]
    fn parses_expr_ternary_v48() {
        parse_program("BEGIN{1?1:1}").unwrap();
    }
    #[test]
    fn parses_expr_in_v48() {
        parse_program("BEGIN{1 in a}").unwrap();
    }

    #[test]
    fn parses_builtin_sin_v48() {
        parse_program("BEGIN{sin(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_cos_v48() {
        parse_program("BEGIN{cos(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_exp_v48() {
        parse_program("BEGIN{exp(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_log_v48() {
        parse_program("BEGIN{log(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_sqrt_v48() {
        parse_program("BEGIN{sqrt(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_int_v48() {
        parse_program("BEGIN{int(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_atan2_v48() {
        parse_program("BEGIN{atan2(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_rand_v48() {
        parse_program("BEGIN{rand()}").unwrap();
    }
    #[test]
    fn parses_builtin_srand_v48_0() {
        parse_program("BEGIN{srand()}").unwrap();
    }
    #[test]
    fn parses_builtin_srand_v48_1() {
        parse_program("BEGIN{srand(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_length_v48_0() {
        parse_program("BEGIN{length()}").unwrap();
    }
    #[test]
    fn parses_builtin_length_v48_1() {
        parse_program("BEGIN{length(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_substr_v48_0() {
        parse_program("BEGIN{substr(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_substr_v48_1() {
        parse_program("BEGIN{substr(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_index_v48() {
        parse_program("BEGIN{index(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_match_v48_0() {
        parse_program("BEGIN{match(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_match_v48_1() {
        parse_program("BEGIN{match(1,1,a)}").unwrap();
    }
    #[test]
    fn parses_builtin_split_v48_0() {
        parse_program("BEGIN{split(1,a)}").unwrap();
    }
    #[test]
    fn parses_builtin_split_v48_1() {
        parse_program("BEGIN{split(1,a,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_split_v48_2() {
        parse_program("BEGIN{split(1,a,1,a)}").unwrap();
    }
    #[test]
    fn parses_builtin_sub_v48_0() {
        parse_program("BEGIN{sub(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_sub_v48_1() {
        parse_program("BEGIN{sub(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_gsub_v48_0() {
        parse_program("BEGIN{gsub(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_gsub_v48_1() {
        parse_program("BEGIN{gsub(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_gensub_v48_0() {
        parse_program("BEGIN{gensub(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_gensub_v48_1() {
        parse_program("BEGIN{gensub(1,1,1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_sprintf_v48() {
        parse_program("BEGIN{sprintf(\"\")}").unwrap();
    }
    #[test]
    fn parses_builtin_strftime_v48_0() {
        parse_program("BEGIN{strftime()}").unwrap();
    }
    #[test]
    fn parses_builtin_strftime_v48_1() {
        parse_program("BEGIN{strftime(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_strftime_v48_2() {
        parse_program("BEGIN{strftime(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_strftime_v48_3() {
        parse_program("BEGIN{strftime(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_mktime_v48_0() {
        parse_program("BEGIN{mktime(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_mktime_v48_1() {
        parse_program("BEGIN{mktime(1,1)}").unwrap();
    }
    #[test]
    fn parses_builtin_systime_v48() {
        parse_program("BEGIN{systime()}").unwrap();
    }
    #[test]
    fn parses_builtin_tolower_v48() {
        parse_program("BEGIN{tolower(1)}").unwrap();
    }
    #[test]
    fn parses_builtin_toupper_v48() {
        parse_program("BEGIN{toupper(1)}").unwrap();
    }

    #[test]
    fn parses_k_begin_v58() {
        parse_program("BEGIN{}").unwrap();
    }
    #[test]
    fn parses_k_end_v58() {
        parse_program("END{}").unwrap();
    }
    #[test]
    fn parses_k_beginfile_v58() {
        parse_program("BEGINFILE{}").unwrap();
    }
    #[test]
    fn parses_k_endfile_v58() {
        parse_program("ENDFILE{}").unwrap();
    }
    #[test]
    fn parses_k_if_v58() {
        parse_program("BEGIN{if(1){}}").unwrap();
    }
    #[test]
    fn parses_k_else_v58() {
        parse_program("BEGIN{if(1){}else{}}").unwrap();
    }
    #[test]
    fn parses_k_while_v58() {
        parse_program("BEGIN{while(1){}}").unwrap();
    }
    #[test]
    fn parses_k_do_v58() {
        parse_program("BEGIN{do{}while(1)}").unwrap();
    }
    #[test]
    fn parses_k_for_v58() {
        parse_program("BEGIN{for(;;){}}").unwrap();
    }
    #[test]
    fn parses_k_forin_v58() {
        parse_program("BEGIN{for(k in a){}}").unwrap();
    }
    #[test]
    fn parses_k_break_v58() {
        parse_program("BEGIN{while(1)break}").unwrap();
    }
    #[test]
    fn parses_k_continue_v58() {
        parse_program("BEGIN{while(1)continue}").unwrap();
    }
    #[test]
    fn parses_k_delete_v58() {
        parse_program("BEGIN{delete a}").unwrap();
    }
    #[test]
    fn parses_k_exit_v58() {
        parse_program("BEGIN{exit}").unwrap();
    }
    #[test]
    fn parses_k_next_v58() {
        parse_program("{next}").unwrap();
    }
    #[test]
    fn parses_k_nextfile_v58() {
        parse_program("{nextfile}").unwrap();
    }
    #[test]
    fn parses_k_return_v58() {
        parse_program("function f(){return}").unwrap();
    }
    #[test]
    fn parses_k_function_v58() {
        parse_program("function f(){}").unwrap();
    }
    #[test]
    fn parses_k_print_v58() {
        parse_program("BEGIN{print}").unwrap();
    }
    #[test]
    fn parses_k_printf_v58() {
        parse_program("BEGIN{printf \"\"}").unwrap();
    }
    #[test]
    fn parses_k_getline_v58() {
        parse_program("BEGIN{getline}").unwrap();
    }
    #[test]
    fn parses_k_in_v58() {
        parse_program("BEGIN{print (1 in a)}").unwrap();
    }

    #[test]
    fn parses_p_lbrace_v58() {
        parse_program("BEGIN{{}}").unwrap();
    }
    #[test]
    fn parses_p_rbrace_v58() {
        parse_program("BEGIN{{}}").unwrap();
    }
    #[test]
    fn parses_p_lparen_v58() {
        parse_program("BEGIN{print(1)}").unwrap();
    }
    #[test]
    fn parses_p_rparen_v58() {
        parse_program("BEGIN{print(1)}").unwrap();
    }
    #[test]
    fn parses_p_lbracket_v58() {
        parse_program("BEGIN{a[1]=1}").unwrap();
    }
    #[test]
    fn parses_p_rbracket_v58() {
        parse_program("BEGIN{a[1]=1}").unwrap();
    }
    #[test]
    fn parses_p_semi_v58() {
        parse_program("BEGIN{;}").unwrap();
    }
    #[test]
    fn parses_p_comma_v58() {
        parse_program("BEGIN{f(1,2)}").unwrap();
    }
    #[test]
    fn parses_p_colon_v58() {
        parse_program("BEGIN{1?1:1}").unwrap();
    }
    #[test]
    fn parses_p_question_v58() {
        parse_program("BEGIN{1?1:1}").unwrap();
    }
    #[test]
    fn parses_p_at_v58() {
        parse_program("BEGIN{@f()}").unwrap();
    }
    #[test]
    fn parses_p_dollar_v58() {
        parse_program("BEGIN{print $1}").unwrap();
    }

    #[test]
    fn parses_o_plus_v58() {
        parse_program("BEGIN{1+1}").unwrap();
    }
    #[test]
    fn parses_o_minus_v58() {
        parse_program("BEGIN{1-1}").unwrap();
    }
    #[test]
    fn parses_o_star_v58() {
        parse_program("BEGIN{1*1}").unwrap();
    }
    #[test]
    fn parses_o_slash_v58() {
        parse_program("BEGIN{1/1}").unwrap();
    }
    #[test]
    fn parses_o_percent_v58() {
        parse_program("BEGIN{1%1}").unwrap();
    }
    #[test]
    fn parses_o_caret_v58() {
        parse_program("BEGIN{1^1}").unwrap();
    }
    #[test]
    fn parses_o_assign_v58() {
        parse_program("BEGIN{x=1}").unwrap();
    }
    #[test]
    fn parses_o_lt_v58() {
        parse_program("BEGIN{1<1}").unwrap();
    }
    #[test]
    fn parses_o_gt_v58() {
        parse_program("BEGIN{1>1}").unwrap();
    }
    #[test]
    fn parses_o_bang_v58() {
        parse_program("BEGIN{!1}").unwrap();
    }
    #[test]
    fn parses_o_tilde_v58() {
        parse_program("BEGIN{1~1}").unwrap();
    }

    #[test]
    fn parses_m_plusplus_v58() {
        parse_program("BEGIN{++x}").unwrap();
    }
    #[test]
    fn parses_m_minusminus_v58() {
        parse_program("BEGIN{--x}").unwrap();
    }
    #[test]
    fn parses_m_pow_v58() {
        parse_program("BEGIN{1**1}").unwrap();
    }
    #[test]
    fn parses_m_eq_v58() {
        parse_program("BEGIN{1==1}").unwrap();
    }
    #[test]
    fn parses_m_ne_v58() {
        parse_program("BEGIN{1!=1}").unwrap();
    }
    #[test]
    fn parses_m_le_v58() {
        parse_program("BEGIN{1<=1}").unwrap();
    }
    #[test]
    fn parses_m_ge_v58() {
        parse_program("BEGIN{1>=1}").unwrap();
    }
    #[test]
    fn parses_m_and_v58() {
        parse_program("BEGIN{1&&1}").unwrap();
    }
    #[test]
    fn parses_m_or_v58() {
        parse_program("BEGIN{1||1}").unwrap();
    }
    #[test]
    fn parses_m_notmatch_v58() {
        parse_program("BEGIN{1!~1}").unwrap();
    }

    #[test]
    fn parses_m_add_assign_v58() {
        parse_program("BEGIN{x+=1}").unwrap();
    }
    #[test]
    fn parses_m_sub_assign_v58() {
        parse_program("BEGIN{x-=1}").unwrap();
    }
    #[test]
    fn parses_m_mul_assign_v58() {
        parse_program("BEGIN{x*=1}").unwrap();
    }
    #[test]
    fn parses_m_div_assign_v58() {
        parse_program("BEGIN{x/=1}").unwrap();
    }
    #[test]
    fn parses_m_mod_assign_v58() {
        parse_program("BEGIN{x%=1}").unwrap();
    }
    #[test]
    fn parses_m_pow_assign_v58() {
        parse_program("BEGIN{x^=1}").unwrap();
    }
    #[test]
    fn parses_m_pow_assign_starstar_v58() {
        parse_program("BEGIN{x**=1}").unwrap();
    }

    #[test]
    fn parses_ba_sin_v60() {
        parse_program("BEGIN{sin(1)}").unwrap();
    }
    #[test]
    fn parses_ba_cos_v60() {
        parse_program("BEGIN{cos(1)}").unwrap();
    }
    #[test]
    fn parses_ba_exp_v60() {
        parse_program("BEGIN{exp(1)}").unwrap();
    }
    #[test]
    fn parses_ba_log_v60() {
        parse_program("BEGIN{log(1)}").unwrap();
    }
    #[test]
    fn parses_ba_sqrt_v60() {
        parse_program("BEGIN{sqrt(1)}").unwrap();
    }
    #[test]
    fn parses_ba_int_v60() {
        parse_program("BEGIN{int(1)}").unwrap();
    }
    #[test]
    fn parses_ba_atan2_v60() {
        parse_program("BEGIN{atan2(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_rand_v60() {
        parse_program("BEGIN{rand()}").unwrap();
    }
    #[test]
    fn parses_ba_srand_v60_0() {
        parse_program("BEGIN{srand()}").unwrap();
    }
    #[test]
    fn parses_ba_srand_v60_1() {
        parse_program("BEGIN{srand(1)}").unwrap();
    }
    #[test]
    fn parses_ba_length_v60_0() {
        parse_program("BEGIN{length()}").unwrap();
    }
    #[test]
    fn parses_ba_length_v60_1() {
        parse_program("BEGIN{length(1)}").unwrap();
    }
    #[test]
    fn parses_ba_substr_v60_0() {
        parse_program("BEGIN{substr(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_substr_v60_1() {
        parse_program("BEGIN{substr(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_index_v60() {
        parse_program("BEGIN{index(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_match_v60_0() {
        parse_program("BEGIN{match(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_match_v60_1() {
        parse_program("BEGIN{match(1,1,a)}").unwrap();
    }
    #[test]
    fn parses_ba_split_v60_0() {
        parse_program("BEGIN{split(1,a)}").unwrap();
    }
    #[test]
    fn parses_ba_split_v60_1() {
        parse_program("BEGIN{split(1,a,1)}").unwrap();
    }
    #[test]
    fn parses_ba_split_v60_2() {
        parse_program("BEGIN{split(1,a,1,a)}").unwrap();
    }
    #[test]
    fn parses_ba_sub_v60_0() {
        parse_program("BEGIN{sub(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_sub_v60_1() {
        parse_program("BEGIN{sub(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_gsub_v60_0() {
        parse_program("BEGIN{gsub(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_gsub_v60_1() {
        parse_program("BEGIN{gsub(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_gensub_v60_0() {
        parse_program("BEGIN{gensub(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_gensub_v60_1() {
        parse_program("BEGIN{gensub(1,1,1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_sprintf_v60() {
        parse_program("BEGIN{sprintf(\"\")}").unwrap();
    }
    #[test]
    fn parses_ba_strftime_v60_0() {
        parse_program("BEGIN{strftime()}").unwrap();
    }
    #[test]
    fn parses_ba_strftime_v60_1() {
        parse_program("BEGIN{strftime(1)}").unwrap();
    }
    #[test]
    fn parses_ba_strftime_v60_2() {
        parse_program("BEGIN{strftime(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_strftime_v60_3() {
        parse_program("BEGIN{strftime(1,1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_mktime_v60_0() {
        parse_program("BEGIN{mktime(1)}").unwrap();
    }
    #[test]
    fn parses_ba_mktime_v60_1() {
        parse_program("BEGIN{mktime(1,1)}").unwrap();
    }
    #[test]
    fn parses_ba_systime_v60() {
        parse_program("BEGIN{systime()}").unwrap();
    }
    #[test]
    fn parses_ba_tolower_v60() {
        parse_program("BEGIN{tolower(1)}").unwrap();
    }
    #[test]
    fn parses_ba_toupper_v60() {
        parse_program("BEGIN{toupper(1)}").unwrap();
    }

    #[test]
    fn parses_ex_sin_v61() {
        parse_program("BEGIN{sin(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_cos_v61() {
        parse_program("BEGIN{cos(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_exp_v61() {
        parse_program("BEGIN{exp(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_log_v61() {
        parse_program("BEGIN{log(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_sqrt_v61() {
        parse_program("BEGIN{sqrt(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_int_v61() {
        parse_program("BEGIN{int(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_atan2_v61() {
        parse_program("BEGIN{atan2(1+1,1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_rand_v61() {
        parse_program("BEGIN{rand()}").unwrap();
    }
    #[test]
    fn parses_ex_srand_v61() {
        parse_program("BEGIN{srand(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_length_v61() {
        parse_program("BEGIN{length(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_substr_v61() {
        parse_program("BEGIN{substr(1+1,1+1,1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_index_v61() {
        parse_program("BEGIN{index(1+1,1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_match_v61() {
        parse_program("BEGIN{match(1+1,1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_sprintf_v61() {
        parse_program("BEGIN{sprintf(\"\",1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_strftime_v61() {
        parse_program("BEGIN{strftime(\"\",1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_mktime_v61() {
        parse_program("BEGIN{mktime(\"\",1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_tolower_v61() {
        parse_program("BEGIN{tolower(1+1)}").unwrap();
    }
    #[test]
    fn parses_ex_toupper_v61() {
        parse_program("BEGIN{toupper(1+1)}").unwrap();
    }

    #[test]
    fn parses_b_sin_v68_0() {
        parse_program("BEGIN{sin(1)}").unwrap();
    }
    #[test]
    fn parses_b_sin_v68_1() {
        parse_program("BEGIN{sin(x)}").unwrap();
    }
    #[test]
    fn parses_b_cos_v68_0() {
        parse_program("BEGIN{cos(1)}").unwrap();
    }
    #[test]
    fn parses_b_cos_v68_1() {
        parse_program("BEGIN{cos(x)}").unwrap();
    }
    #[test]
    fn parses_b_exp_v68_0() {
        parse_program("BEGIN{exp(1)}").unwrap();
    }
    #[test]
    fn parses_b_exp_v68_1() {
        parse_program("BEGIN{exp(x)}").unwrap();
    }
    #[test]
    fn parses_b_log_v68_0() {
        parse_program("BEGIN{log(1)}").unwrap();
    }
    #[test]
    fn parses_b_log_v68_1() {
        parse_program("BEGIN{log(x)}").unwrap();
    }
    #[test]
    fn parses_b_sqrt_v68_0() {
        parse_program("BEGIN{sqrt(1)}").unwrap();
    }
    #[test]
    fn parses_b_sqrt_v68_1() {
        parse_program("BEGIN{sqrt(x)}").unwrap();
    }
    #[test]
    fn parses_b_int_v68_0() {
        parse_program("BEGIN{int(1)}").unwrap();
    }
    #[test]
    fn parses_b_int_v68_1() {
        parse_program("BEGIN{int(x)}").unwrap();
    }
    #[test]
    fn parses_b_atan2_v68_0() {
        parse_program("BEGIN{atan2(1,1)}").unwrap();
    }
    #[test]
    fn parses_b_atan2_v68_1() {
        parse_program("BEGIN{atan2(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_rand_v68_0() {
        parse_program("BEGIN{rand()}").unwrap();
    }
    #[test]
    fn parses_b_srand_v68_0() {
        parse_program("BEGIN{srand()}").unwrap();
    }
    #[test]
    fn parses_b_srand_v68_1() {
        parse_program("BEGIN{srand(x)}").unwrap();
    }
    #[test]
    fn parses_b_length_v68_0() {
        parse_program("BEGIN{length()}").unwrap();
    }
    #[test]
    fn parses_b_length_v68_1() {
        parse_program("BEGIN{length(x)}").unwrap();
    }
    #[test]
    fn parses_b_substr_v68_0() {
        parse_program("BEGIN{substr(x,1)}").unwrap();
    }
    #[test]
    fn parses_b_substr_v68_1() {
        parse_program("BEGIN{substr(x,1,1)}").unwrap();
    }
    #[test]
    fn parses_b_index_v68_0() {
        parse_program("BEGIN{index(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_match_v68_0() {
        parse_program("BEGIN{match(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_match_v68_1() {
        parse_program("BEGIN{match(x,y,a)}").unwrap();
    }
    #[test]
    fn parses_b_split_v68_0() {
        parse_program("BEGIN{split(x,a)}").unwrap();
    }
    #[test]
    fn parses_b_split_v68_1() {
        parse_program("BEGIN{split(x,a,y)}").unwrap();
    }
    #[test]
    fn parses_b_split_v68_2() {
        parse_program("BEGIN{split(x,a,y,seps)}").unwrap();
    }
    #[test]
    fn parses_b_sub_v68_0() {
        parse_program("BEGIN{sub(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_sub_v68_1() {
        parse_program("BEGIN{sub(x,y,z)}").unwrap();
    }
    #[test]
    fn parses_b_gsub_v68_0() {
        parse_program("BEGIN{gsub(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_gsub_v68_1() {
        parse_program("BEGIN{gsub(x,y,z)}").unwrap();
    }
    #[test]
    fn parses_b_gensub_v68_0() {
        parse_program("BEGIN{gensub(x,y,z)}").unwrap();
    }
    #[test]
    fn parses_b_gensub_v68_1() {
        parse_program("BEGIN{gensub(x,y,z,w)}").unwrap();
    }
    #[test]
    fn parses_b_sprintf_v68_0() {
        parse_program("BEGIN{sprintf(x)}").unwrap();
    }
    #[test]
    fn parses_b_sprintf_v68_1() {
        parse_program("BEGIN{sprintf(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_strftime_v68_0() {
        parse_program("BEGIN{strftime()}").unwrap();
    }
    #[test]
    fn parses_b_strftime_v68_1() {
        parse_program("BEGIN{strftime(x)}").unwrap();
    }
    #[test]
    fn parses_b_strftime_v68_2() {
        parse_program("BEGIN{strftime(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_strftime_v68_3() {
        parse_program("BEGIN{strftime(x,y,z)}").unwrap();
    }
    #[test]
    fn parses_b_mktime_v68_0() {
        parse_program("BEGIN{mktime(x)}").unwrap();
    }
    #[test]
    fn parses_b_mktime_v68_1() {
        parse_program("BEGIN{mktime(x,y)}").unwrap();
    }
    #[test]
    fn parses_b_systime_v68_0() {
        parse_program("BEGIN{systime()}").unwrap();
    }
    #[test]
    fn parses_b_tolower_v68_0() {
        parse_program("BEGIN{tolower(x)}").unwrap();
    }
    #[test]
    fn parses_b_toupper_v68_0() {
        parse_program("BEGIN{toupper(x)}").unwrap();
    }

    #[test]
    fn parses_nested_if_v69() {
        parse_program("BEGIN{if(1){if(1){if(1){print 1}}}}").unwrap();
    }
    #[test]
    fn parses_nested_while_v69() {
        parse_program("BEGIN{while(1){while(1){while(1){print 1}}}}").unwrap();
    }
    #[test]
    fn parses_nested_for_v69() {
        parse_program("BEGIN{for(;;){for(;;){for(;;){print 1}}}}").unwrap();
    }
    #[test]
    fn parses_nested_block_v69() {
        parse_program("BEGIN{{{{print 1}}}}").unwrap();
    }

    #[test]
    fn parses_stmt_print_multi_v69() {
        parse_program("BEGIN{print 1, 2, 3, 4, 5}").unwrap();
    }
    #[test]
    fn parses_stmt_printf_multi_v69() {
        parse_program("BEGIN{printf \"%d %d %d\", 1, 2, 3}").unwrap();
    }
}
