use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Begin,
    End,
    BeginFile,
    EndFile,
    Print,
    If,
    Else,
    While,
    For,
    Do,
    Break,
    Continue,
    Next,
    Exit,
    In,
    Function,
    Return,
    Delete,
    Getline,
    Ident(String),
    Number(f64),
    String(String),
    Regexp(String),

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    /// `>>` (append redirect; distinct from two `>` tokens).
    GtGt,
    Ge,
    Assign,
    Pipe,
    And,
    Or,
    Bang,
    Tilde,
    NotTilde,
    Question,
    Colon,
    Semi,
    Comma,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Dollar,
    Newline,
    Eof,
}

#[derive(Clone)]
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
        }
    }

    pub fn line(&self) -> usize {
        self.line
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        let len = c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        self.pos += len;
        Some(c)
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' || c == '\r' {
                self.bump();
            } else if c == '#' {
                while let Some(d) = self.peek() {
                    self.bump();
                    if d == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// When `regex_mode` is true, `/` begins a `/regex/` literal; otherwise `/` is division.
    pub fn next_token(&mut self, regex_mode: bool) -> Result<Token> {
        self.skip_ws();
        let Some(c) = self.peek() else {
            return Ok(Token::Eof);
        };

        if c == '\n' {
            self.bump();
            return Ok(Token::Newline);
        }

        if c == '/' && regex_mode {
            self.bump();
            let mut s = String::new();
            while let Some(d) = self.peek() {
                if d == '/' && !s.ends_with('\\') {
                    self.bump();
                    return Ok(Token::Regexp(s));
                }
                if d == '\n' {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "unterminated regex".into(),
                    });
                }
                s.push(d);
                self.bump();
            }
            return Err(Error::Parse {
                line: self.line,
                msg: "unterminated regex".into(),
            });
        }

        // string
        if c == '"' {
            self.bump();
            let mut s = String::new();
            while let Some(d) = self.peek() {
                if d == '"' {
                    self.bump();
                    return Ok(Token::String(s));
                }
                if d == '\\' {
                    self.bump();
                    match self.peek() {
                        Some('n') => {
                            self.bump();
                            s.push('\n');
                        }
                        Some('t') => {
                            self.bump();
                            s.push('\t');
                        }
                        Some('r') => {
                            self.bump();
                            s.push('\r');
                        }
                        Some('\\') | Some('"') => s.push(self.bump().unwrap()),
                        Some(x) => {
                            self.bump();
                            s.push(x);
                        }
                        None => {
                            return Err(Error::Parse {
                                line: self.line,
                                msg: "unterminated string".into(),
                            });
                        }
                    }
                    continue;
                }
                if d == '\n' {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "newline in string".into(),
                    });
                }
                s.push(d);
                self.bump();
            }
            return Err(Error::Parse {
                line: self.line,
                msg: "unterminated string".into(),
            });
        }

        // number
        if c.is_ascii_digit() || (c == '.' && self.lookahead_digit()) {
            let start = self.pos;
            while let Some(d) = self.peek() {
                if d.is_ascii_digit() {
                    self.bump();
                } else {
                    break;
                }
            }
            if self.peek() == Some('.') {
                self.bump();
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            let slice = &self.input[start..self.pos];
            let n: f64 = slice.parse().map_err(|_| Error::Parse {
                line: self.line,
                msg: format!("bad number {slice:?}"),
            })?;
            return Ok(Token::Number(n));
        }

        // ident / keyword
        if is_ident_start(c) {
            let start = self.pos;
            self.bump();
            while let Some(d) = self.peek() {
                if is_ident_continue(d) {
                    self.bump();
                } else {
                    break;
                }
            }
            let name = self.input[start..self.pos].to_string();
            let tok = match name.as_str() {
                "BEGIN" => Token::Begin,
                "BEGINFILE" => Token::BeginFile,
                "END" => Token::End,
                "ENDFILE" => Token::EndFile,
                "print" => Token::Print,
                "if" => Token::If,
                "else" => Token::Else,
                "while" => Token::While,
                "for" => Token::For,
                "do" => Token::Do,
                "break" => Token::Break,
                "continue" => Token::Continue,
                "next" => Token::Next,
                "exit" => Token::Exit,
                "in" => Token::In,
                "function" => Token::Function,
                "return" => Token::Return,
                "delete" => Token::Delete,
                "getline" => Token::Getline,
                _ => Token::Ident(name),
            };
            return Ok(tok);
        }

        self.bump();
        let t = match c {
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            ';' => Token::Semi,
            ',' => Token::Comma,
            '$' => Token::Dollar,
            '?' => Token::Question,
            ':' => Token::Colon,
            '+' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::AddAssign
                } else {
                    Token::Plus
                }
            }
            '-' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::SubAssign
                } else {
                    Token::Minus
                }
            }
            '*' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::MulAssign
                } else {
                    Token::Star
                }
            }
            '%' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::ModAssign
                } else {
                    Token::Percent
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.bump();
                    Token::Or
                } else {
                    Token::Pipe
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.bump();
                    Token::And
                } else {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "unexpected `&`".into(),
                    });
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::Ne
                } else if self.peek() == Some('~') {
                    self.bump();
                    Token::NotTilde
                } else {
                    Token::Bang
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::Eq
                } else {
                    Token::Assign
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::Le
                } else {
                    Token::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::Ge
                } else if self.peek() == Some('>') {
                    self.bump();
                    Token::GtGt
                } else {
                    Token::Gt
                }
            }
            '~' => Token::Tilde,
            '/' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::DivAssign
                } else {
                    Token::Slash
                }
            }
            _ => {
                return Err(Error::Parse {
                    line: self.line,
                    msg: format!("unexpected character {c:?}"),
                });
            }
        };
        Ok(t)
    }

    fn lookahead_digit(&self) -> bool {
        let i = self.pos + 1;
        if i >= self.input.len() {
            return false;
        }
        self.input[i..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}
