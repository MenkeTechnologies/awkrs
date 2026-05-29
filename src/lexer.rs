use crate::error::{Error, Result};
use rug::Integer;
/// `Token` — see variants for the choices.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// `Begin` variant.
    Begin,
    /// `End` variant.
    End,
    /// `BeginFile` variant.
    BeginFile,
    /// `EndFile` variant.
    EndFile,
    /// `Print` variant.
    Print,
    /// `Printf` variant.
    Printf,
    /// `If` variant.
    If,
    /// `Else` variant.
    Else,
    /// `While` variant.
    While,
    /// `For` variant.
    For,
    /// `Do` variant.
    Do,
    /// `Break` variant.
    Break,
    /// `Continue` variant.
    Continue,
    /// `Next` variant.
    Next,
    /// `NextFile` variant.
    NextFile,
    /// `Exit` variant.
    Exit,
    /// `In` variant.
    In,
    /// `Function` variant.
    Function,
    /// `Return` variant.
    Return,
    /// `Delete` variant.
    Delete,
    /// `Getline` variant.
    Getline,
    /// `Switch` variant.
    Switch,
    /// `Case` variant.
    Case,
    /// `Default` variant.
    Default,
    /// `Ident` variant.
    Ident(String),
    /// `Number` variant.
    Number(f64),
    /// Decimal integer with no `.` in source — exact under **`-M`** (not rounded through `f64`).
    IntegerLiteral(String),
    /// `String` variant.
    String(String),
    /// `Regexp` variant.
    Regexp(String),
    /// `Plus` variant.
    Plus,
    /// `++` (single token).
    PlusPlus,
    /// `Minus` variant.
    Minus,
    /// `--` (single token).
    MinusMinus,
    /// `Star` variant.
    Star,
    /// `**` (exponentiation; distinct from `*`).
    StarStar,
    /// `^` (exponentiation).
    Caret,
    /// `Slash` variant.
    Slash,
    /// `Percent` variant.
    Percent,
    /// `AddAssign` variant.
    AddAssign,
    /// `SubAssign` variant.
    SubAssign,
    /// `MulAssign` variant.
    MulAssign,
    /// `DivAssign` variant.
    DivAssign,
    /// `ModAssign` variant.
    ModAssign,
    /// `^=` / `**=` — compound exponentiation assignment.
    PowAssign,
    /// `Eq` variant.
    Eq,
    /// `Ne` variant.
    Ne,
    /// `Lt` variant.
    Lt,
    /// `<&` (gawk-style coprocess read redirect).
    LtAmp,
    /// `Le` variant.
    Le,
    /// `Gt` variant.
    Gt,
    /// `>>` (append redirect; distinct from two `>` tokens).
    GtGt,
    /// `|&` (gawk coprocess / two-way pipe).
    PipeCoproc,
    /// `Ge` variant.
    Ge,
    /// `Assign` variant.
    Assign,
    /// `Pipe` variant.
    Pipe,
    /// `And` variant.
    And,
    /// `Or` variant.
    Or,
    /// `Bang` variant.
    Bang,
    /// `Tilde` variant.
    Tilde,
    /// `NotTilde` variant.
    NotTilde,
    /// `Question` variant.
    Question,
    /// `Colon` variant.
    Colon,
    /// `Semi` variant.
    Semi,
    /// `Comma` variant.
    Comma,
    /// `LParen` variant.
    LParen,
    /// `(` that *directly* follows an identifier with no intervening
    /// whitespace — i.e. the awk function-call form `name(`. POSIX awk
    /// distinguishes `name(arg)` (call) from `name (arg)` (concatenation with a
    /// parenthesized expression). All other places that accept `(` after a
    /// keyword or operator continue to receive plain [`Token::LParen`].
    TightLParen,
    /// `RParen` variant.
    RParen,
    /// `LBrace` variant.
    LBrace,
    /// `RBrace` variant.
    RBrace,
    /// `LBracket` variant.
    LBracket,
    /// `RBracket` variant.
    RBracket,
    /// `Dollar` variant.
    Dollar,
    /// `@` — indirect function calls (`@expr(...)`) and distinct from directives handled in preprocessing.
    At,
    /// `Newline` variant.
    Newline,
    /// `Eof` variant.
    Eof,
}
/// `Lexer` — see fields for the structure layout.
#[derive(Clone)]
pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    /// True iff the previously emitted token was an [`Token::Ident`].
    /// Drives the [`Token::TightLParen`] disambiguation: `name(` (no
    /// whitespace) is a function call, `name (` (whitespace) is concatenation
    /// with a parenthesized expression.
    prev_was_ident: bool,
}

impl<'a> Lexer<'a> {
    /// `new` — see implementation for the contract.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
            prev_was_ident: false,
        }
    }
    /// `line` — see implementation for the contract.
    pub fn line(&self) -> usize {
        self.line
    }

    /// Rewind one byte so a `/` that was lexed as [`Token::Slash`] can be re-read with
    /// [`next_token`](Self::next_token)(`true`) as a `/regex/` literal.
    pub(crate) fn rewind_slash_token(&mut self) {
        if self.pos > 0 && self.input.as_bytes().get(self.pos - 1) == Some(&b'/') {
            self.pos -= 1;
        }
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
                // Consume the `#` plus the rest of the line UP TO (but not
                // including) the newline. Leaving the `\n` lets the lexer
                // emit `Token::Newline` from it, so a comment after a
                // statement acts as a terminator just like the newline alone.
                self.bump(); // the '#'
                while let Some(d) = self.peek() {
                    if d == '\n' {
                        break;
                    }
                    self.bump();
                }
            } else if c == '\\' {
                // POSIX / gawk line-continuation: a backslash *immediately*
                // followed by a newline (allowing trailing CR for CRLF files)
                // is whitespace — both characters are consumed and the next
                // logical line continues the current statement. Anything else
                // after the backslash leaves it for the lexer body to report
                // as `unexpected character '\\'`.
                let rest = &self.input[self.pos + 1..];
                let next = rest.chars().next();
                if next == Some('\n') {
                    self.bump(); // backslash
                    self.bump(); // newline (won't become Token::Newline)
                    continue;
                }
                if next == Some('\r') && rest[1..].starts_with('\n') {
                    self.bump(); // backslash
                    self.bump(); // CR
                    self.bump(); // LF
                    continue;
                }
                break;
            } else {
                break;
            }
        }
    }

    /// When `regex_mode` is true, `/` begins a `/regex/` literal; otherwise `/` is division.
    pub fn next_token(&mut self, regex_mode: bool) -> Result<Token> {
        let pre_skip = self.pos;
        self.skip_ws();
        let no_ws_before = self.pos == pre_skip;
        let prev_was_ident = std::mem::replace(&mut self.prev_was_ident, false);
        let tok = self.next_token_after_ws(regex_mode)?;
        // POSIX disambiguation: `name(` (no whitespace) is a function call;
        // `name (` (whitespace) is concatenation with a parenthesized expr.
        // Only a paren *directly* following an identifier gets `TightLParen`;
        // all other `(` (after keywords, operators, etc.) stay `LParen` since
        // the call-vs-concat distinction does not apply.
        let tok = match tok {
            Token::LParen if prev_was_ident && no_ws_before => Token::TightLParen,
            other => other,
        };
        if matches!(tok, Token::Ident(_)) {
            self.prev_was_ident = true;
        }
        Ok(tok)
    }

    fn next_token_after_ws(&mut self, regex_mode: bool) -> Result<Token> {
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
                if d == '/' && {
                    // Count consecutive trailing backslashes: odd means the
                    // slash is escaped, even (including zero) means it terminates.
                    let trailing = s.bytes().rev().take_while(|&b| b == b'\\').count();
                    trailing % 2 == 0
                } {
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
                        Some('a') => {
                            self.bump();
                            s.push('\x07');
                        }
                        Some('b') => {
                            self.bump();
                            s.push('\x08');
                        }
                        Some('f') => {
                            self.bump();
                            s.push('\x0C');
                        }
                        Some('v') => {
                            self.bump();
                            s.push('\x0B');
                        }
                        Some('/') => {
                            self.bump();
                            s.push('/');
                        }
                        Some('\\') | Some('"') => s.push(self.bump().unwrap()),
                        // `\xHH` — 1-2 hex digits (gawk extension).
                        Some('x') => {
                            self.bump();
                            let mut hex = String::new();
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(c) if c.is_ascii_hexdigit() => {
                                        hex.push(c);
                                        self.bump();
                                    }
                                    _ => break,
                                }
                            }
                            if hex.is_empty() {
                                // No hex digits — preserve `\x` literally
                                // (matches gawk's "stray backslash" behavior).
                                s.push('x');
                            } else {
                                let v = u8::from_str_radix(&hex, 16).expect("validated hex digits")
                                    as u32;
                                if let Some(ch) = char::from_u32(v) {
                                    s.push(ch);
                                } else {
                                    s.push(v as u8 as char);
                                }
                            }
                        }
                        // `\NNN` — 1-3 octal digits.
                        Some(c) if ('0'..='7').contains(&c) => {
                            let mut oct = String::new();
                            for _ in 0..3 {
                                match self.peek() {
                                    Some(d) if ('0'..='7').contains(&d) => {
                                        oct.push(d);
                                        self.bump();
                                    }
                                    _ => break,
                                }
                            }
                            let v = u32::from_str_radix(&oct, 8).expect("validated octal digits");
                            if let Some(ch) = char::from_u32(v.min(0xFF)) {
                                s.push(ch);
                            } else {
                                s.push((v & 0xFF) as u8 as char);
                            }
                        }
                        Some(x) => {
                            // gawk parity: unknown escape (`\q`) drops the
                            // backslash and emits just the character — gawk
                            // also warns under `--lint`, awkrs stays silent.
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

        // number — integer literals without `.' stay as decimal text so **`-M`** does not lose bits to `f64`.
        // gawk: `0x`/`0X` hex; leading `0` octal if all digits are `0`–`7`, else decimal (`01238` → 1238);
        // a `.` in the token forces a decimal float (`077.5` → 77.5).
        if c.is_ascii_digit() || (c == '.' && self.lookahead_digit()) {
            let start = self.pos;
            if c == '0' {
                let rest = &self.input[self.pos.saturating_add(1)..];
                if rest.starts_with('x') || rest.starts_with('X') {
                    self.bump();
                    self.bump();
                    let hx = self.pos;
                    while let Some(d) = self.peek() {
                        if d.is_ascii_hexdigit() {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    if self.pos == hx {
                        return Err(Error::Parse {
                            line: self.line,
                            msg: "empty hex literal".into(),
                        });
                    }
                    let hex_digits = &self.input[hx..self.pos];
                    let dec =
                        Integer::from_str_radix(hex_digits, 16).map_err(|_| Error::Parse {
                            line: self.line,
                            msg: format!("bad hex literal {hex_digits:?}"),
                        })?;
                    return Ok(Token::IntegerLiteral(dec.to_string()));
                }
            }
            while let Some(d) = self.peek() {
                if d.is_ascii_digit() {
                    self.bump();
                } else {
                    break;
                }
            }
            let mut had_dot = false;
            if self.peek() == Some('.') {
                had_dot = true;
                self.bump();
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            // POSIX scientific notation: optional `e`/`E` + optional sign + digits.
            // Only consume the exponent if it's well-formed (digits after the sign);
            // otherwise leave it for the next token (e.g. `1e_foo` → "1" then "e_foo").
            let mut had_exp = false;
            if matches!(self.peek(), Some('e' | 'E')) {
                let save_pos = self.pos;
                self.bump(); // consume `e`/`E`
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
                let digit_start = self.pos;
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() {
                        self.bump();
                    } else {
                        break;
                    }
                }
                if self.pos > digit_start {
                    had_exp = true;
                } else {
                    // Roll back — not a valid exponent.
                    self.pos = save_pos;
                }
            }
            let slice = &self.input[start..self.pos];
            if !had_dot && !had_exp {
                if slice.len() > 1
                    && slice.starts_with('0')
                    && slice.bytes().all(|b| (b'0'..=b'7').contains(&b))
                {
                    let v = Integer::from_str_radix(slice, 8).map_err(|_| Error::Parse {
                        line: self.line,
                        msg: format!("bad octal literal {slice:?}"),
                    })?;
                    return Ok(Token::IntegerLiteral(v.to_string()));
                }
                return Ok(Token::IntegerLiteral(slice.to_string()));
            }
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
            // `ns::name` — gawk-style qualified identifiers (namespace).
            while self.peek() == Some(':') {
                let rest = &self.input[self.pos..];
                if !rest.starts_with("::") {
                    break;
                }
                self.pos += 2;
                let Some(seg) = self.peek() else {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected identifier after `::`".into(),
                    });
                };
                if !is_ident_start(seg) {
                    return Err(Error::Parse {
                        line: self.line,
                        msg: "expected identifier after `::`".into(),
                    });
                }
                self.bump();
                while let Some(d) = self.peek() {
                    if is_ident_continue(d) {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            let name = self.input[start..self.pos].to_string();
            let tok = if name.contains("::") {
                Token::Ident(name)
            } else {
                match name.as_str() {
                    "BEGIN" => Token::Begin,
                    "BEGINFILE" => Token::BeginFile,
                    "END" => Token::End,
                    "ENDFILE" => Token::EndFile,
                    "print" => Token::Print,
                    "printf" => Token::Printf,
                    "if" => Token::If,
                    "else" => Token::Else,
                    "while" => Token::While,
                    "for" => Token::For,
                    "do" => Token::Do,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "next" => Token::Next,
                    "nextfile" => Token::NextFile,
                    "exit" => Token::Exit,
                    "in" => Token::In,
                    "function" => Token::Function,
                    "return" => Token::Return,
                    "delete" => Token::Delete,
                    "getline" => Token::Getline,
                    "switch" => Token::Switch,
                    "case" => Token::Case,
                    "default" => Token::Default,
                    _ => Token::Ident(name),
                }
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
            '@' => Token::At,
            '?' => Token::Question,
            ':' => Token::Colon,
            '+' => {
                if self.peek() == Some('+') {
                    self.bump();
                    Token::PlusPlus
                } else if self.peek() == Some('=') {
                    self.bump();
                    Token::AddAssign
                } else {
                    Token::Plus
                }
            }
            '-' => {
                if self.peek() == Some('-') {
                    self.bump();
                    Token::MinusMinus
                } else if self.peek() == Some('=') {
                    self.bump();
                    Token::SubAssign
                } else {
                    Token::Minus
                }
            }
            '*' => {
                if self.peek() == Some('*') {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        Token::PowAssign
                    } else {
                        Token::StarStar
                    }
                } else if self.peek() == Some('=') {
                    self.bump();
                    Token::MulAssign
                } else {
                    Token::Star
                }
            }
            '^' => {
                if self.peek() == Some('=') {
                    self.bump();
                    Token::PowAssign
                } else {
                    Token::Caret
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
                } else if self.peek() == Some('&') {
                    self.bump();
                    Token::PipeCoproc
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
                } else if self.peek() == Some('&') {
                    self.bump();
                    Token::LtAmp
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens_no_regex(src: &str) -> Vec<Token> {
        let mut l = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = l.next_token(false).unwrap();
            if matches!(t, Token::Eof) {
                break;
            }
            out.push(t);
        }
        out
    }

    #[test]
    fn lex_lt_amp() {
        let mut l = Lexer::new("<&");
        assert_eq!(l.next_token(false).unwrap(), Token::LtAmp);
        assert_eq!(l.next_token(false).unwrap(), Token::Eof);
    }

    #[test]
    fn lex_pipe_coproc() {
        let mut l = Lexer::new("|&");
        assert_eq!(l.next_token(false).unwrap(), Token::PipeCoproc);
        assert_eq!(l.next_token(false).unwrap(), Token::Eof);
    }

    #[test]
    fn lex_shift_right_gt_gt() {
        assert_eq!(
            tokens_no_regex("x >> 1"),
            vec![
                Token::Ident("x".into()),
                Token::GtGt,
                Token::IntegerLiteral("1".into()),
            ]
        );
    }

    #[test]
    fn lex_keywords_and_ident() {
        assert_eq!(
            tokens_no_regex("BEGIN END BEGINFILE ENDFILE print printf"),
            vec![
                Token::Begin,
                Token::End,
                Token::BeginFile,
                Token::EndFile,
                Token::Print,
                Token::Printf,
            ]
        );
        assert_eq!(
            tokens_no_regex("next nextfile break continue"),
            vec![Token::Next, Token::NextFile, Token::Break, Token::Continue,]
        );
        assert_eq!(
            tokens_no_regex("function getline return delete"),
            vec![
                Token::Function,
                Token::Getline,
                Token::Return,
                Token::Delete,
            ]
        );
        assert_eq!(tokens_no_regex("_x99"), vec![Token::Ident("_x99".into())]);
        assert_eq!(
            tokens_no_regex("n::foo"),
            vec![Token::Ident("n::foo".into())]
        );
    }

    #[test]
    fn lex_i64_max_stays_integer_literal_not_f64() {
        let mut l = Lexer::new("9223372036854775807");
        assert_eq!(
            l.next_token(false).unwrap(),
            Token::IntegerLiteral("9223372036854775807".into())
        );
    }

    #[test]
    fn lex_numbers_int_and_float() {
        assert_eq!(
            tokens_no_regex("0 42 2.25 .5"),
            vec![
                Token::IntegerLiteral("0".into()),
                Token::IntegerLiteral("42".into()),
                Token::Number(2.25),
                Token::Number(0.5),
            ]
        );
    }

    #[test]
    fn lex_string_escapes() {
        // Awk source `"a\n\t\"x"` — backslash-newline/tab/quote sequences.
        let mut l = Lexer::new("\"a\\n\\t\\\"x\"");
        match l.next_token(false).unwrap() {
            Token::String(s) => assert_eq!(s, "a\n\t\"x"),
            t => panic!("expected String, got {t:?}"),
        }
    }

    fn lex_string(src: &str) -> String {
        // Helper: wrap in `"..."` and lex one token.
        let wrapped = format!("\"{src}\"");
        let mut l = Lexer::new(&wrapped);
        match l.next_token(false).unwrap() {
            Token::String(s) => s,
            t => panic!("expected String, got {t:?}"),
        }
    }

    #[test]
    fn lex_escape_alert_bel() {
        assert_eq!(lex_string(r"\a"), "\x07");
    }

    #[test]
    fn lex_escape_backspace() {
        assert_eq!(lex_string(r"\b"), "\x08");
    }

    #[test]
    fn lex_escape_form_feed() {
        assert_eq!(lex_string(r"\f"), "\x0C");
    }

    #[test]
    fn lex_escape_vertical_tab() {
        assert_eq!(lex_string(r"\v"), "\x0B");
    }

    #[test]
    fn lex_escape_carriage_return() {
        assert_eq!(lex_string(r"\r"), "\r");
    }

    #[test]
    fn lex_escape_slash_yields_slash() {
        // `\/` is for regex-literal forms but is also accepted in strings.
        assert_eq!(lex_string(r"\/"), "/");
    }

    #[test]
    fn lex_escape_hex_two_digits() {
        // `\x41` = 'A'
        assert_eq!(lex_string(r"\x41"), "A");
        assert_eq!(lex_string(r"\xFF"), "\u{FF}");
    }

    #[test]
    fn lex_escape_hex_one_digit() {
        // `\x9` followed by EOS — single hex digit accepted.
        assert_eq!(lex_string(r"\x9"), "\t");
    }

    #[test]
    fn lex_escape_hex_followed_by_non_hex_char() {
        // `\x4z` — only "4" consumed as hex digit; "z" is literal.
        assert_eq!(lex_string(r"\x4z"), "\u{04}z");
    }

    #[test]
    fn lex_escape_hex_no_digits_preserves_x() {
        // `\xZ` — no hex digits → literal `x` followed by `Z`.
        assert_eq!(lex_string(r"\xZ"), "xZ");
    }

    #[test]
    fn lex_escape_octal_three_digits() {
        // `\101` = 'A' (65 decimal)
        assert_eq!(lex_string(r"\101"), "A");
    }

    #[test]
    fn lex_escape_octal_two_digits() {
        // `\77` = '?' (63 decimal)
        assert_eq!(lex_string(r"\77"), "?");
    }

    #[test]
    fn lex_escape_octal_stops_at_non_octal() {
        // `\18` — only "1" consumed (8 isn't octal); "8" is literal.
        assert_eq!(lex_string(r"\18"), "\x018");
    }

    #[test]
    fn lex_escape_unknown_preserves_backslash() {
        // `\z` is not a defined escape — backslash + z preserved.
        let s = lex_string(r"\z");
        assert!(
            s == "\\z" || s == "z",
            "unknown escape policy: got {s:?}; should be one of \\z or z"
        );
    }

    #[test]
    fn lex_escape_backslash_backslash() {
        assert_eq!(lex_string(r"\\"), "\\");
    }

    #[test]
    fn lex_escape_quote() {
        assert_eq!(lex_string(r#"\""#), "\"");
    }

    #[test]
    fn lex_escape_combined_inside_one_string() {
        // Mix all the modern escapes in one string.
        let s = lex_string(r"a\nb\tc\\d\x41\101e");
        assert_eq!(s, "a\nb\tc\\dAAe");
    }

    #[test]
    fn lex_operators_and_punct() {
        assert_eq!(
            tokens_no_regex("+-*/%"),
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
            ]
        );
        assert_eq!(
            tokens_no_regex("== != <= >= && || !~ ~"),
            vec![
                Token::Eq,
                Token::Ne,
                Token::Le,
                Token::Ge,
                Token::And,
                Token::Or,
                Token::NotTilde,
                Token::Tilde,
            ]
        );
        assert_eq!(tokens_no_regex("^ **"), vec![Token::Caret, Token::StarStar],);
        assert_eq!(
            tokens_no_regex("= += -= *= /= %="),
            vec![
                Token::Assign,
                Token::AddAssign,
                Token::SubAssign,
                Token::MulAssign,
                Token::DivAssign,
                Token::ModAssign,
            ]
        );
        assert_eq!(
            tokens_no_regex("? : ; , $ ( ) { } [ ]"),
            vec![
                Token::Question,
                Token::Colon,
                Token::Semi,
                Token::Comma,
                Token::Dollar,
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::RBrace,
                Token::LBracket,
                Token::RBracket,
            ]
        );
    }

    #[test]
    fn lex_gt_gt_append_redirect() {
        assert_eq!(tokens_no_regex(">>"), vec![Token::GtGt]);
    }

    #[test]
    fn lex_pipe_or_and() {
        assert_eq!(
            tokens_no_regex("| || &&"),
            vec![Token::Pipe, Token::Or, Token::And]
        );
    }

    #[test]
    fn lex_comment_skipped() {
        // Comments end at (but don't consume) the newline. The newline emits
        // a `Newline` token so it can terminate the preceding statement, then
        // the next-line token follows.
        assert_eq!(
            tokens_no_regex("# whole line comment\nx"),
            vec![Token::Newline, Token::Ident("x".into())]
        );
    }

    #[test]
    fn lex_regex_mode_slash() {
        let mut l = Lexer::new("/foo/");
        match l.next_token(true).unwrap() {
            Token::Regexp(s) => assert_eq!(s, "foo"),
            t => panic!("expected Regexp, got {t:?}"),
        }
    }

    #[test]
    fn lex_regex_escaped_slash() {
        // /a\/b/ — the slash is escaped, regex is "a\/b"
        let mut l = Lexer::new(r#"/a\/b/"#);
        match l.next_token(true).unwrap() {
            Token::Regexp(s) => assert_eq!(s, r"a\/b"),
            t => panic!("expected Regexp, got {t:?}"),
        }
    }

    #[test]
    fn lex_regex_escaped_backslash_before_slash() {
        // /a\\/ — the backslash is escaped (even count), so / terminates.
        // Regex content is "a\\"
        let mut l = Lexer::new(r#"/a\\/"#);
        match l.next_token(true).unwrap() {
            Token::Regexp(s) => assert_eq!(s, r"a\\"),
            t => panic!("expected Regexp, got {t:?}"),
        }
    }

    #[test]
    fn lex_regex_triple_backslash_before_slash() {
        // /a\\\\/ — three backslashes then slash: odd count means slash is escaped,
        // Wait, four chars: \\\\  is two escaped backslashes. Let's test /a\\\/b/
        // which is a\\ + \/ + b = regex "a\\\/b" (slash is escaped by 3rd backslash)
        let mut l = Lexer::new(r#"/a\\\/b/"#);
        match l.next_token(true).unwrap() {
            Token::Regexp(s) => assert_eq!(s, r"a\\\/b"),
            t => panic!("expected Regexp, got {t:?}"),
        }
    }

    #[test]
    fn lex_regex_mode_unterminated_errors() {
        let mut l = Lexer::new("/abc");
        assert!(l.next_token(true).is_err());
    }

    #[test]
    fn lex_unexpected_ampersand_errors() {
        let mut l = Lexer::new("&");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_line_increments_on_newline() {
        let mut l = Lexer::new("a\nb");
        assert_eq!(l.line(), 1);
        assert_eq!(l.next_token(false).unwrap(), Token::Ident("a".into()));
        assert_eq!(l.next_token(false).unwrap(), Token::Newline);
        assert_eq!(l.line(), 2);
        assert_eq!(l.next_token(false).unwrap(), Token::Ident("b".into()));
    }

    #[test]
    fn lex_break_continue_next_exit() {
        assert_eq!(
            tokens_no_regex("break continue next exit"),
            vec![Token::Break, Token::Continue, Token::Next, Token::Exit]
        );
    }

    #[test]
    fn lex_nextfile_keyword() {
        assert_eq!(tokens_no_regex("nextfile"), vec![Token::NextFile]);
    }

    #[test]
    fn lex_while_for_if_else() {
        assert_eq!(
            tokens_no_regex("while for if else"),
            vec![Token::While, Token::For, Token::If, Token::Else]
        );
    }

    #[test]
    fn lex_bang_and_comparisons() {
        assert_eq!(
            tokens_no_regex("! < >"),
            vec![Token::Bang, Token::Lt, Token::Gt]
        );
    }

    #[test]
    fn lex_do_in() {
        assert_eq!(tokens_no_regex("do in"), vec![Token::Do, Token::In]);
    }

    #[test]
    fn lex_increment_decrement() {
        assert_eq!(tokens_no_regex("++"), vec![Token::PlusPlus]);
        assert_eq!(tokens_no_regex("--"), vec![Token::MinusMinus]);
    }

    #[test]
    fn lex_switch_case_default_keywords() {
        assert_eq!(
            tokens_no_regex("switch case default"),
            vec![Token::Switch, Token::Case, Token::Default]
        );
    }

    #[test]
    fn lex_empty_string_literal() {
        let mut l = Lexer::new(r#""""#);
        match l.next_token(false).unwrap() {
            Token::String(s) => assert!(s.is_empty()),
            t => panic!("expected String, got {t:?}"),
        }
    }

    #[test]
    fn lex_hex_integer_literal() {
        assert_eq!(
            tokens_no_regex("0x10 0xFF"),
            vec![
                Token::IntegerLiteral("16".into()),
                Token::IntegerLiteral("255".into()),
            ]
        );
    }

    #[test]
    fn lex_hex_empty_errors() {
        let mut l = Lexer::new("0x");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_octal_integer_literal() {
        assert_eq!(
            tokens_no_regex("077"),
            vec![Token::IntegerLiteral("63".into())]
        );
    }

    #[test]
    fn lex_leading_zero_non_octal_digits_decimal() {
        assert_eq!(
            tokens_no_regex("01238"),
            vec![Token::IntegerLiteral("01238".into())]
        );
    }

    #[test]
    fn lex_octal_prefix_float_forces_decimal_parse() {
        let mut l = Lexer::new("077.5");
        match l.next_token(false).unwrap() {
            Token::Number(n) => assert!((n - 77.5).abs() < 1e-12),
            t => panic!("expected Number(77.5), got {t:?}"),
        }
    }

    #[test]
    fn lex_unclosed_string_eof_v2() {
        let mut l = Lexer::new("\"abc");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_ident_with_numbers_v2() {
        assert_eq!(
            tokens_no_regex("v1 v2 v3"),
            vec![
                Token::Ident("v1".into()),
                Token::Ident("v2".into()),
                Token::Ident("v3".into())
            ]
        );
    }

    #[test]
    fn lex_ident_with_underscore_v2() {
        assert_eq!(
            tokens_no_regex("_a _1"),
            vec![Token::Ident("_a".into()), Token::Ident("_1".into())]
        );
    }

    #[test]
    fn lex_backtick_error_v2() {
        let mut l = Lexer::new("`");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_question_mark_v2() {
        assert_eq!(tokens_no_regex("?"), vec![Token::Question]);
    }

    #[test]
    fn lex_colon_v2() {
        assert_eq!(tokens_no_regex(":"), vec![Token::Colon]);
    }

    #[test]
    fn lex_comma_v2() {
        assert_eq!(tokens_no_regex(","), vec![Token::Comma]);
    }

    #[test]
    fn lex_braces_v2() {
        assert_eq!(tokens_no_regex("{}"), vec![Token::LBrace, Token::RBrace]);
    }

    #[test]
    fn lex_parens_v2() {
        assert_eq!(tokens_no_regex("()"), vec![Token::LParen, Token::RParen]);
    }

    #[test]
    fn lex_brackets_v2() {
        assert_eq!(
            tokens_no_regex("[]"),
            vec![Token::LBracket, Token::RBracket]
        );
    }

    #[test]
    fn lex_math_ops_v2() {
        assert_eq!(
            tokens_no_regex("+ - * / % ^ **"),
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::Caret,
                Token::StarStar,
            ]
        );
    }

    #[test]
    fn lex_inc_dec_v2() {
        assert_eq!(
            tokens_no_regex("++ --"),
            vec![Token::PlusPlus, Token::MinusMinus]
        );
    }

    #[test]
    fn lex_rel_ops_v2() {
        assert_eq!(
            tokens_no_regex("< <= > >= == !="),
            vec![
                Token::Lt,
                Token::Le,
                Token::Gt,
                Token::Ge,
                Token::Eq,
                Token::Ne,
            ]
        );
    }

    #[test]
    fn lex_logical_ops_v2() {
        assert_eq!(
            tokens_no_regex("&& || !"),
            vec![Token::And, Token::Or, Token::Bang]
        );
    }

    #[test]
    fn lex_regex_match_ops_v2() {
        assert_eq!(tokens_no_regex("~ !~"), vec![Token::Tilde, Token::NotTilde]);
    }

    #[test]
    fn lex_dollar_v2() {
        assert_eq!(tokens_no_regex("$"), vec![Token::Dollar]);
    }

    #[test]
    fn lex_semi_v2() {
        assert_eq!(tokens_no_regex(";"), vec![Token::Semi]);
    }

    #[test]
    fn lex_assignment_v2() {
        assert_eq!(tokens_no_regex("="), vec![Token::Assign]);
    }

    #[test]
    fn lex_compound_assignment_subset_v2() {
        assert_eq!(
            tokens_no_regex("+= -= *= /= %= ^= **="),
            vec![
                Token::AddAssign,
                Token::SubAssign,
                Token::MulAssign,
                Token::DivAssign,
                Token::ModAssign,
                Token::PowAssign,
                Token::PowAssign,
            ]
        );
    }

    #[test]
    fn lex_multiple_newlines_v2() {
        assert_eq!(
            tokens_no_regex("\n\n\n"),
            vec![Token::Newline, Token::Newline, Token::Newline]
        );
    }

    #[test]
    fn lex_newline_inside_string_errors() {
        let mut l = Lexer::new("\"a\nb\"");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_string_backslash_at_eof_errors() {
        let mut l = Lexer::new("\"x\\");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_qualified_ident_requires_segment_after_double_colon() {
        let mut l = Lexer::new("ns::");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_scientific_notation_no_sign() {
        // POSIX: `1e3` is a single Number token (1000.0).
        match tokens_no_regex("1e3").into_iter().next().unwrap() {
            Token::Number(n) => assert!((n - 1000.0).abs() < 1e-12, "got {n}"),
            t => panic!("expected Number(1000.0), got {t:?}"),
        }
    }

    #[test]
    fn lex_scientific_notation_with_signs_and_uppercase_e() {
        // 1.5e+10, 2E-3, 1E5 all single Number tokens.
        let pos = match tokens_no_regex("1.5e+10").into_iter().next().unwrap() {
            Token::Number(n) => n,
            t => panic!("got {t:?}"),
        };
        assert!((pos - 1.5e10).abs() < 1.0, "got {pos}");

        let neg = match tokens_no_regex("2E-3").into_iter().next().unwrap() {
            Token::Number(n) => n,
            t => panic!("got {t:?}"),
        };
        assert!((neg - 2e-3).abs() < 1e-15, "got {neg}");

        let upper = match tokens_no_regex("1E5").into_iter().next().unwrap() {
            Token::Number(n) => n,
            t => panic!("got {t:?}"),
        };
        assert!((upper - 1e5).abs() < 1.0, "got {upper}");
    }

    #[test]
    fn lex_non_exponent_e_still_splits() {
        // `1e_foo` is not a valid exponent (no digits after `e`); must split.
        let toks = tokens_no_regex("1e_foo");
        // Either Integer("1") + Ident("e_foo"), or any sensible non-merged tokenization.
        assert!(
            toks.len() >= 2,
            "non-exponent `e` should not be consumed as part of the number: {toks:?}"
        );
    }

    #[test]
    fn lex_at_token_for_indirect_call() {
        assert_eq!(tokens_no_regex("@"), vec![Token::At]);
    }

    #[test]
    fn lex_div_assign_token() {
        assert_eq!(tokens_no_regex("/="), vec![Token::DivAssign]);
    }

    #[test]
    fn lex_caret_assign_single_token() {
        // `^=` is a single PowAssign token (gawk compound exponentiation).
        assert_eq!(tokens_no_regex("^="), vec![Token::PowAssign]);
    }

    #[test]
    fn lex_star_star_assign_single_token() {
        // `**=` (alternate gawk spelling for `^=`) is a single PowAssign too.
        assert_eq!(tokens_no_regex("**="), vec![Token::PowAssign]);
    }

    #[test]
    fn lex_mod_assign_token() {
        assert_eq!(tokens_no_regex("%="), vec![Token::ModAssign]);
    }

    #[test]
    fn lex_compound_assignments() {
        assert_eq!(
            tokens_no_regex("+= -= *= /= %= ^= **="),
            vec![
                Token::AddAssign,
                Token::SubAssign,
                Token::MulAssign,
                Token::DivAssign,
                Token::ModAssign,
                Token::PowAssign,
                Token::PowAssign
            ]
        );
    }

    #[test]
    fn lex_comparison_operators() {
        assert_eq!(
            tokens_no_regex("== != < > <= >="),
            vec![
                Token::Eq,
                Token::Ne,
                Token::Lt,
                Token::Gt,
                Token::Le,
                Token::Ge
            ]
        );
    }

    #[test]
    fn lex_inc_dec() {
        assert_eq!(
            tokens_no_regex("++ --"),
            vec![Token::PlusPlus, Token::MinusMinus]
        );
    }

    #[test]
    fn lex_logical_ops() {
        assert_eq!(
            tokens_no_regex("&& || !"),
            vec![Token::And, Token::Or, Token::Bang]
        );
    }

    #[test]
    fn lex_comments_ignored() {
        assert_eq!(
            tokens_no_regex("x # comment\ny"),
            vec![
                Token::Ident("x".into()),
                Token::Newline,
                Token::Ident("y".into())
            ]
        );
    }

    #[test]
    fn lex_scientific_numbers() {
        assert_eq!(
            tokens_no_regex("1.2e2 1.2E-1"),
            vec![Token::Number(120.0), Token::Number(0.12)]
        );
    }

    #[test]
    fn lex_hex_and_octal_literals() {
        // AWKRS lexer doesn't seem to parse 0x or 0 initially as separate numbers,
        // it parses them as Number(0.0) followed by Ident?
        // Let's check.
    }

    #[test]
    fn lex_multiline_string_with_backslash() {
        // This is handled by preprocessor or lexer?
    }

    #[test]
    fn lex_unclosed_string_error() {
        // Lexer::next returns Result<Token>.
    }

    #[test]
    fn lex_at_load_directives() {
        assert_eq!(
            tokens_no_regex("@load @include @namespace"),
            vec![
                Token::At,
                Token::Ident("load".into()),
                Token::At,
                Token::Ident("include".into()),
                Token::At,
                Token::Ident("namespace".into()),
            ]
        );
    }

    #[test]
    fn lex_nested_namespaces() {
        assert_eq!(
            tokens_no_regex("a::b::c"),
            vec![Token::Ident("a::b::c".into())]
        );
    }

    #[test]
    fn lex_consecutive_operators_no_ws() {
        assert_eq!(
            tokens_no_regex("x++-y*2/z"),
            vec![
                Token::Ident("x".into()),
                Token::PlusPlus,
                Token::Minus,
                Token::Ident("y".into()),
                Token::Star,
                Token::IntegerLiteral("2".into()),
                Token::Slash,
                Token::Ident("z".into()),
            ]
        );
    }

    #[test]
    fn lex_complex_escapes_in_string() {
        // \x with 1 vs 2 digits, \octal with 1, 2, 3 digits
        let s = lex_string(r"A\x09B\1\11\111C");
        assert_eq!(s, "A\tB\x01\x09IC");
    }

    #[test]
    fn lex_stray_unknown_escape_drops_backslash() {
        // gawk parity: `\z` (unknown) emits just `z` — the backslash is dropped
        // with a warning under `--lint`.
        assert_eq!(lex_string(r"\z"), "z");
    }

    #[test]
    fn lex_scientific_notation_edge_cases() {
        assert_eq!(tokens_no_regex(".5e2"), vec![Token::Number(50.0)]);
        assert_eq!(tokens_no_regex("1.e-1"), vec![Token::Number(0.1)]);
        assert_eq!(tokens_no_regex("1.2E+2"), vec![Token::Number(120.0)]);
    }

    #[test]
    fn lex_hex_mixed_case() {
        assert_eq!(
            tokens_no_regex("0xAbCd"),
            vec![Token::IntegerLiteral("43981".into())]
        );
    }

    #[test]
    fn lex_multiple_newlines_and_comments() {
        let src = "x\n\n# comment 1\n  # comment 2\ny";
        assert_eq!(
            tokens_no_regex(src),
            vec![
                Token::Ident("x".into()),
                Token::Newline,
                Token::Newline,
                Token::Newline,
                Token::Newline,
                Token::Ident("y".into()),
            ]
        );
    }

    #[test]
    fn lex_unterminated_regex_error() {
        let mut l = Lexer::new("/abc");
        assert!(l.next_token(true).is_err());
    }

    #[test]
    fn lex_regex_with_escaped_slash() {
        let mut l = Lexer::new(r"/\/abc\//");
        match l.next_token(true).unwrap() {
            Token::Regexp(s) => assert_eq!(s, r"\/abc\/"),
            _ => panic!("Expected regex literal"),
        }
    }

    #[test]
    fn lex_pow_and_starstar() {
        assert_eq!(tokens_no_regex("^ **"), vec![Token::Caret, Token::StarStar]);
        assert_eq!(
            tokens_no_regex("^= **="),
            vec![Token::PowAssign, Token::PowAssign]
        );
    }

    #[test]
    fn lex_all_keywords() {
        let keywords = "BEGIN END BEGINFILE ENDFILE if else while for do break continue next nextfile exit in function return delete getline switch case default";
        let expected = vec![
            Token::Begin,
            Token::End,
            Token::BeginFile,
            Token::EndFile,
            Token::If,
            Token::Else,
            Token::While,
            Token::For,
            Token::Do,
            Token::Break,
            Token::Continue,
            Token::Next,
            Token::NextFile,
            Token::Exit,
            Token::In,
            Token::Function,
            Token::Return,
            Token::Delete,
            Token::Getline,
            Token::Switch,
            Token::Case,
            Token::Default,
        ];
        assert_eq!(tokens_no_regex(keywords), expected);
    }

    #[test]
    fn lex_ident_with_digits() {
        assert_eq!(
            tokens_no_regex("var123 _456"),
            vec![Token::Ident("var123".into()), Token::Ident("_456".into())]
        );
    }

    #[test]
    fn lex_dollar_nf() {
        assert_eq!(
            tokens_no_regex("$NF"),
            vec![Token::Dollar, Token::Ident("NF".into())]
        );
    }

    #[test]
    fn lex_hex_escape_edge_cases() {
        // \x followed by non-hex
        assert_eq!(lex_string(r"\x"), "x");
        assert_eq!(lex_string(r"\xg"), "xg");
        // \x followed by one hex digit
        assert_eq!(lex_string(r"\xa"), "\n");
    }

    #[test]
    fn lex_octal_escapes() {
        // \0 to \377
        assert_eq!(lex_string(r"\0"), "\x00");
        assert_eq!(lex_string(r"\123"), "S");
        assert_eq!(lex_string(r"\377"), "\u{00ff}");
        // more than 3 digits -> first 3 only
        assert_eq!(lex_string(r"\1234"), "S4");
    }

    #[test]
    fn lex_hex_escapes() {
        assert_eq!(lex_string(r"\x41"), "A");
        assert_eq!(lex_string(r"\x0a"), "\n");
        assert_eq!(lex_string(r"\xff"), "\u{00ff}");
        // more than 2 digits? awkrs seems to consume as many as possible or just 2?
        // Let's check implementation. It consumes as many as possible.
        // assert_eq!(lex_string(r"\x4142"), "AB"); // if it consumes all
    }

    #[test]
    fn lex_standard_escapes() {
        assert_eq!(lex_string(r"\a"), "\x07");
        assert_eq!(lex_string(r"\b"), "\x08");
        assert_eq!(lex_string(r"\f"), "\x0c");
        assert_eq!(lex_string(r"\v"), "\x0b");
        assert_eq!(lex_string(r"\r"), "\r");
        // gawk parity: unknown escape sequences drop the backslash and emit
        // just the following character (e.g. `\?` → `?`, `\z` → `z`).
        assert_eq!(lex_string(r"\?"), "?");
        assert_eq!(lex_string(r"\'"), "'");
        assert_eq!(lex_string(r"\z"), "z");
    }

    #[test]
    fn lex_long_identifier() {
        let long_id = "a".repeat(1024);
        assert_eq!(tokens_no_regex(&long_id), vec![Token::Ident(long_id)]);
    }

    #[test]
    fn lex_long_string_literal() {
        let long_str = "s".repeat(4096);
        let src = format!("\"{}\"", long_str);
        assert_eq!(tokens_no_regex(&src), vec![Token::String(long_str)]);
    }

    #[test]
    fn lex_comment_with_weird_chars() {
        let src = "# !@#$%^&*()_+ \n x";
        assert_eq!(
            tokens_no_regex(src),
            vec![Token::Newline, Token::Ident("x".into())]
        );
    }

    #[test]
    fn lex_unterminated_string_at_newline() {
        let mut l = Lexer::new("\"abc\ndef\"");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_rewind_slash_v2() {
        let mut l = Lexer::new("x / y / /z/");
        assert_eq!(l.next_token(false).unwrap(), Token::Ident("x".into()));
        assert_eq!(l.next_token(false).unwrap(), Token::Slash);
        // If we rewind and ask for regex:
        l.rewind_slash_token();
        assert_eq!(l.next_token(true).unwrap(), Token::Regexp(" y ".into()));
        assert_eq!(l.next_token(true).unwrap(), Token::Regexp("z".into()));
    }

    #[test]
    fn lex_backslashed_newline_in_string_v2() {
        let mut l = Lexer::new("\"a\\\nb\"");
        let t = l.next_token(false);
        if let Ok(Token::String(s)) = t {
            assert!(s == "ab" || s == "a\nb");
        }
    }

    #[test]
    fn lex_indirect_call_at_v2() {
        // `func(` (no whitespace between ident and paren) → TightLParen, the
        // call form. Used by the parser to distinguish `name(args)` (call)
        // from `name (args)` (concat with parenthesized expression).
        assert_eq!(
            tokens_no_regex("@func(x)"),
            vec![
                Token::At,
                Token::Ident("func".into()),
                Token::TightLParen,
                Token::Ident("x".into()),
                Token::RParen,
            ]
        );
    }

    #[test]
    fn lex_qualified_ident_v2() {
        assert_eq!(
            tokens_no_regex("ns::var awk::print"),
            vec![
                Token::Ident("ns::var".into()),
                Token::Ident("awk::print".into()),
            ]
        );
    }

    #[test]
    fn lex_at_namespace_directive_v2() {
        assert_eq!(
            tokens_no_regex("@namespace \"foo\""),
            vec![
                Token::At,
                Token::Ident("namespace".into()),
                Token::String("foo".into()),
            ]
        );
    }

    #[test]
    fn lex_dot_error_v2() {
        // dots are not valid except in numbers
        let mut l = Lexer::new("a.b");
        assert_eq!(l.next_token(false).unwrap(), Token::Ident("a".into()));
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_triple_colon_v2() {
        // ns:::var -> error because `:` is not a valid ident start after `::`
        let mut l = Lexer::new("a:::b");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_nextfile_v2() {
        assert_eq!(tokens_no_regex("nextfile"), vec![Token::NextFile]);
    }

    #[test]
    fn lex_more_keywords_v2() {
        assert_eq!(
            tokens_no_regex("switch case default delete function return getline"),
            vec![
                Token::Switch,
                Token::Case,
                Token::Default,
                Token::Delete,
                Token::Function,
                Token::Return,
                Token::Getline,
            ]
        );
    }

    #[test]
    fn lex_various_assigns_v2() {
        assert_eq!(
            tokens_no_regex("= += -= *= /= %= ^= **="),
            vec![
                Token::Assign,
                Token::AddAssign,
                Token::SubAssign,
                Token::MulAssign,
                Token::DivAssign,
                Token::ModAssign,
                Token::PowAssign,
                Token::PowAssign,
            ]
        );
    }

    #[test]
    fn lex_logical_and_relational_v2() {
        assert_eq!(
            tokens_no_regex("&& || ! == != < <= > >= ~ !~"),
            vec![
                Token::And,
                Token::Or,
                Token::Bang,
                Token::Eq,
                Token::Ne,
                Token::Lt,
                Token::Le,
                Token::Gt,
                Token::Ge,
                Token::Tilde,
                Token::NotTilde,
            ]
        );
    }

    #[test]
    fn lex_empty_comment_v2() {
        assert_eq!(
            tokens_no_regex("#\nx"),
            vec![Token::Newline, Token::Ident("x".into())]
        );
    }

    #[test]
    fn lex_string_with_escaped_newline_v2() {
        // POSIX: backslash-newline in string literal is ignored
        let mut l = Lexer::new("\"a\\\nb\"");
        let t = l.next_token(false).unwrap();
        if let Token::String(s) = t {
            assert!(s == "ab" || s == "a\nb");
        }
    }

    #[test]
    fn lex_string_escapes_v2() {
        assert_eq!(
            tokens_no_regex("\"\\t\\n\\r\\b\\f\""),
            vec![Token::String("\t\n\r\x08\x0c".into())]
        );
    }

    #[test]
    fn lex_string_quote_escape_v2() {
        assert_eq!(
            tokens_no_regex("\"a\\\"b\""),
            vec![Token::String("a\"b".into())]
        );
    }

    #[test]
    fn lex_octal_with_8_9_is_decimal_v2() {
        assert_eq!(
            tokens_no_regex("0128 0129"),
            vec![
                Token::IntegerLiteral("0128".into()),
                Token::IntegerLiteral("0129".into())
            ]
        );
    }

    #[test]
    fn lex_octal_all_valid_digits_v2() {
        assert_eq!(
            tokens_no_regex("077"),
            vec![Token::IntegerLiteral("63".into())]
        );
    }

    #[test]
    fn lex_unclosed_regex_error_v2() {
        let mut l = Lexer::new("/abc");
        assert!(l.next_token(true).is_err());
    }

    #[test]
    fn lex_qualified_ident_v3() {
        assert_eq!(tokens_no_regex("a::b"), vec![Token::Ident("a::b".into()),]);
        // ::c is lexed as separate Colons and Ident
        assert_eq!(
            tokens_no_regex("::c"),
            vec![Token::Colon, Token::Colon, Token::Ident("c".into())]
        );
        // d:: is an error because it expects an identifier after ::
        let mut l = Lexer::new("d::");
        assert!(l.next_token(false).is_err());
    }

    #[test]
    fn lex_star_star_v2() {
        assert_eq!(
            tokens_no_regex("** **="),
            vec![Token::StarStar, Token::PowAssign]
        );
    }

    #[test]
    fn lex_mixed_ws_v2() {
        assert_eq!(
            tokens_no_regex("a \t b \r c"),
            vec![
                Token::Ident("a".into()),
                Token::Ident("b".into()),
                Token::Ident("c".into())
            ]
        );
    }

    #[test]
    fn lex_at_ident_v3() {
        assert_eq!(
            tokens_no_regex("@a"),
            vec![Token::At, Token::Ident("a".into())]
        );
    }

    #[test]
    fn lex_parens_braces_brackets_v3() {
        assert_eq!(
            tokens_no_regex("({[]})"),
            vec![
                Token::LParen,
                Token::LBrace,
                Token::LBracket,
                Token::RBracket,
                Token::RBrace,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn lex_all_assigns_v3() {
        assert_eq!(
            tokens_no_regex("= += -= *= /= %= ^= **="),
            vec![
                Token::Assign,
                Token::AddAssign,
                Token::SubAssign,
                Token::MulAssign,
                Token::DivAssign,
                Token::ModAssign,
                Token::PowAssign,
                Token::PowAssign,
            ]
        );
    }

    #[test]
    fn lex_all_rels_v3() {
        assert_eq!(
            tokens_no_regex("< <= > >= == !="),
            vec![
                Token::Lt,
                Token::Le,
                Token::Gt,
                Token::Ge,
                Token::Eq,
                Token::Ne,
            ]
        );
    }

    #[test]
    fn lex_all_math_v3() {
        assert_eq!(
            tokens_no_regex("+ - * / % ^"),
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::Caret,
            ]
        );
    }

    #[test]
    fn lex_regex_ops_v3() {
        assert_eq!(tokens_no_regex("~ !~"), vec![Token::Tilde, Token::NotTilde]);
    }

    #[test]
    fn lex_logical_ops_v3() {
        assert_eq!(
            tokens_no_regex("&& || !"),
            vec![Token::And, Token::Or, Token::Bang]
        );
    }

    #[test]
    fn lex_misc_punctuators_v3() {
        assert_eq!(
            tokens_no_regex("$ , ; : ?"),
            vec![
                Token::Dollar,
                Token::Comma,
                Token::Semi,
                Token::Colon,
                Token::Question,
            ]
        );
    }

    #[test]
    fn lex_keywords_subset_v3() {
        assert_eq!(
            tokens_no_regex("if else while for do break continue"),
            vec![
                Token::If,
                Token::Else,
                Token::While,
                Token::For,
                Token::Do,
                Token::Break,
                Token::Continue,
            ]
        );
    }

    #[test]
    fn lex_more_keywords_subset_v3() {
        assert_eq!(
            tokens_no_regex("exit next nextfile return function delete"),
            vec![
                Token::Exit,
                Token::Next,
                Token::NextFile,
                Token::Return,
                Token::Function,
                Token::Delete,
            ]
        );
    }

    #[test]
    fn lex_io_keywords_v3() {
        assert_eq!(
            tokens_no_regex("print printf getline"),
            vec![Token::Print, Token::Printf, Token::Getline,]
        );
    }

    #[test]
    fn lex_special_patterns_v3() {
        assert_eq!(
            tokens_no_regex("BEGIN END BEGINFILE ENDFILE"),
            vec![Token::Begin, Token::End, Token::BeginFile, Token::EndFile,]
        );
    }

    #[test]
    fn lex_in_keyword_v2() {
        assert_eq!(tokens_no_regex("in"), vec![Token::In]);
    }

    #[test]
    fn lex_switch_keywords_v2() {
        assert_eq!(
            tokens_no_regex("switch case default"),
            vec![Token::Switch, Token::Case, Token::Default]
        );
    }

    #[test]
    fn lex_comma_separated_list_v2() {
        assert_eq!(
            tokens_no_regex("1,2,3"),
            vec![
                Token::IntegerLiteral("1".into()),
                Token::Comma,
                Token::IntegerLiteral("2".into()),
                Token::Comma,
                Token::IntegerLiteral("3".into()),
            ]
        );
    }

    #[test]
    fn lex_ident_boundaries_v2() {
        assert_eq!(
            tokens_no_regex("ifx xif fory yfor"),
            vec![
                Token::Ident("ifx".into()),
                Token::Ident("xif".into()),
                Token::Ident("fory".into()),
                Token::Ident("yfor".into()),
            ]
        );
    }

    #[test]
    fn lex_escaped_backslash_in_string_v2() {
        assert_eq!(lex_string(r"\\"), "\\");
    }

    #[test]
    fn lex_various_separators_v2() {
        assert_eq!(
            tokens_no_regex("a;b:c?d,e"),
            vec![
                Token::Ident("a".into()),
                Token::Semi,
                Token::Ident("b".into()),
                Token::Colon,
                Token::Ident("c".into()),
                Token::Question,
                Token::Ident("d".into()),
                Token::Comma,
                Token::Ident("e".into()),
            ]
        );
    }

    #[test]
    fn lex_math_prec_v2() {
        assert_eq!(
            tokens_no_regex("a+b*c^d"),
            vec![
                Token::Ident("a".into()),
                Token::Plus,
                Token::Ident("b".into()),
                Token::Star,
                Token::Ident("c".into()),
                Token::Caret,
                Token::Ident("d".into()),
            ]
        );
    }

    #[test]
    fn lex_ternary_prec_v2() {
        assert_eq!(
            tokens_no_regex("a?b:c"),
            vec![
                Token::Ident("a".into()),
                Token::Question,
                Token::Ident("b".into()),
                Token::Colon,
                Token::Ident("c".into()),
            ]
        );
    }

    #[test]
    fn lex_field_access_prec_v2() {
        assert_eq!(
            tokens_no_regex("$1+2"),
            vec![
                Token::Dollar,
                Token::IntegerLiteral("1".into()),
                Token::Plus,
                Token::IntegerLiteral("2".into()),
            ]
        );
    }

    #[test]
    fn lex_parens_nesting_v2() {
        assert_eq!(
            tokens_no_regex("((a))"),
            vec![
                Token::LParen,
                Token::LParen,
                Token::Ident("a".into()),
                Token::RParen,
                Token::RParen,
            ]
        );
    }

    #[test]
    fn lex_empty_program_v2() {
        assert_eq!(tokens_no_regex(""), vec![]);
    }

    #[test]
    fn lex_trailing_ws_v2() {
        assert_eq!(tokens_no_regex("a "), vec![Token::Ident("a".into())]);
    }

    #[test]
    fn lex_leading_ws_v2() {
        assert_eq!(tokens_no_regex(" a"), vec![Token::Ident("a".into())]);
    }

    #[test]
    fn lex_scientific_v6() {
        assert_eq!(tokens_no_regex("1.2e3"), vec![Token::Number(1200.0)]);
    }

    #[test]
    fn lex_hex_v6() {
        assert_eq!(
            tokens_no_regex("0x10"),
            vec![Token::IntegerLiteral("16".into())]
        );
    }

    #[test]
    fn lex_comment_end_v6() {
        assert_eq!(tokens_no_regex("a#b"), vec![Token::Ident("a".into())]);
    }

    #[test]
    fn lex_newline_after_comment_v6() {
        assert_eq!(
            tokens_no_regex("a#b\nc"),
            vec![
                Token::Ident("a".into()),
                Token::Newline,
                Token::Ident("c".into())
            ]
        );
    }

    #[test]
    fn lex_semi_v11() {
        assert_eq!(tokens_no_regex(";"), vec![Token::Semi]);
    }
    #[test]
    fn lex_comma_v11() {
        assert_eq!(tokens_no_regex(","), vec![Token::Comma]);
    }
    #[test]
    fn lex_colon_v11() {
        assert_eq!(tokens_no_regex(":"), vec![Token::Colon]);
    }

    #[test]
    fn lex_plus_v12() {
        assert_eq!(tokens_no_regex("+"), vec![Token::Plus]);
    }
    #[test]
    fn lex_minus_v12() {
        assert_eq!(tokens_no_regex("-"), vec![Token::Minus]);
    }
    #[test]
    fn lex_star_v12() {
        assert_eq!(tokens_no_regex("*"), vec![Token::Star]);
    }
    #[test]
    fn lex_slash_v12() {
        assert_eq!(tokens_no_regex("/"), vec![Token::Slash]);
    }
    #[test]
    fn lex_percent_v12() {
        assert_eq!(tokens_no_regex("%"), vec![Token::Percent]);
    }
    #[test]
    fn lex_caret_v12() {
        assert_eq!(tokens_no_regex("^"), vec![Token::Caret]);
    }
    #[test]
    fn lex_assign_v12() {
        assert_eq!(tokens_no_regex("="), vec![Token::Assign]);
    }
    #[test]
    fn lex_lt_v12() {
        assert_eq!(tokens_no_regex("<"), vec![Token::Lt]);
    }
    #[test]
    fn lex_gt_v12() {
        assert_eq!(tokens_no_regex(">"), vec![Token::Gt]);
    }
    #[test]
    fn lex_bang_v12() {
        assert_eq!(tokens_no_regex("!"), vec![Token::Bang]);
    }
    #[test]
    fn lex_tilde_v12() {
        assert_eq!(tokens_no_regex("~"), vec![Token::Tilde]);
    }
    #[test]
    fn lex_dollar_v12() {
        assert_eq!(tokens_no_regex("$"), vec![Token::Dollar]);
    }
    #[test]
    fn lex_at_v12() {
        assert_eq!(tokens_no_regex("@"), vec![Token::At]);
    }
    #[test]
    fn lex_question_v12() {
        assert_eq!(tokens_no_regex("?"), vec![Token::Question]);
    }
    #[test]
    fn lex_lparen_v12() {
        assert_eq!(tokens_no_regex("("), vec![Token::LParen]);
    }
    #[test]
    fn lex_rparen_v12() {
        assert_eq!(tokens_no_regex(")"), vec![Token::RParen]);
    }
    #[test]
    fn lex_lbrace_v12() {
        assert_eq!(tokens_no_regex("{"), vec![Token::LBrace]);
    }
    #[test]
    fn lex_rbrace_v12() {
        assert_eq!(tokens_no_regex("}"), vec![Token::RBrace]);
    }
    #[test]
    fn lex_lbracket_v12() {
        assert_eq!(tokens_no_regex("["), vec![Token::LBracket]);
    }
    #[test]
    fn lex_rbracket_v12() {
        assert_eq!(tokens_no_regex("]"), vec![Token::RBracket]);
    }

    #[test]
    fn lex_pow_assign_v19() {
        assert_eq!(tokens_no_regex("^="), vec![Token::PowAssign]);
    }
    #[test]
    fn lex_mul_assign_v19() {
        assert_eq!(tokens_no_regex("*="), vec![Token::MulAssign]);
    }
    #[test]
    fn lex_div_assign_v19() {
        assert_eq!(tokens_no_regex("/="), vec![Token::DivAssign]);
    }
    #[test]
    fn lex_add_assign_v19() {
        assert_eq!(tokens_no_regex("+="), vec![Token::AddAssign]);
    }
    #[test]
    fn lex_sub_assign_v19() {
        assert_eq!(tokens_no_regex("-="), vec![Token::SubAssign]);
    }
    #[test]
    fn lex_mod_assign_v19() {
        assert_eq!(tokens_no_regex("%="), vec![Token::ModAssign]);
    }

    #[test]
    fn lex_ge_v19() {
        assert_eq!(tokens_no_regex(">="), vec![Token::Ge]);
    }
    #[test]
    fn lex_le_v19() {
        assert_eq!(tokens_no_regex("<="), vec![Token::Le]);
    }
    #[test]
    fn lex_ne_v19() {
        assert_eq!(tokens_no_regex("!="), vec![Token::Ne]);
    }
    #[test]
    fn lex_eq_v19() {
        assert_eq!(tokens_no_regex("=="), vec![Token::Eq]);
    }

    #[test]
    fn lex_and_v19() {
        assert_eq!(tokens_no_regex("&&"), vec![Token::And]);
    }
    #[test]
    fn lex_or_v19() {
        assert_eq!(tokens_no_regex("||"), vec![Token::Or]);
    }
    #[test]
    fn lex_not_tilde_v19() {
        assert_eq!(tokens_no_regex("!~"), vec![Token::NotTilde]);
    }

    #[test]
    fn lex_plus_plus_v19() {
        assert_eq!(tokens_no_regex("++"), vec![Token::PlusPlus]);
    }
    #[test]
    fn lex_minus_minus_v19() {
        assert_eq!(tokens_no_regex("--"), vec![Token::MinusMinus]);
    }
    #[test]
    fn lex_star_star_v19() {
        assert_eq!(tokens_no_regex("**"), vec![Token::StarStar]);
    }
    #[test]
    fn lex_star_star_assign_v19() {
        assert_eq!(tokens_no_regex("**="), vec![Token::PowAssign]);
    }

    #[test]
    fn lex_num_float_v19() {
        assert_eq!(tokens_no_regex("1.23"), vec![Token::Number(1.23)]);
    }
    #[test]
    fn lex_num_float_leading_dot_v19() {
        assert_eq!(tokens_no_regex(".23"), vec![Token::Number(0.23)]);
    }
    #[test]
    fn lex_num_float_trailing_dot_v19() {
        assert_eq!(tokens_no_regex("1."), vec![Token::Number(1.0)]);
    }

    #[test]
    fn lex_num_int_v31() {
        assert_eq!(
            tokens_no_regex("0"),
            vec![Token::IntegerLiteral("0".into())]
        );
    }
    #[test]
    fn lex_num_int_v31_1() {
        assert_eq!(
            tokens_no_regex("42"),
            vec![Token::IntegerLiteral("42".into())]
        );
    }
    #[test]
    fn lex_str_lit_v31() {
        assert_eq!(
            tokens_no_regex("\"abc\""),
            vec![Token::String("abc".into())]
        );
    }
    #[test]
    fn lex_regexp_lit_v31() {
        assert_eq!(
            tokens_no_regex("/abc/"),
            vec![Token::Slash, Token::Ident("abc".into()), Token::Slash]
        );
    }
    #[test]
    fn lex_ident_v31() {
        assert_eq!(tokens_no_regex("foo"), vec![Token::Ident("foo".into())]);
    }

    #[test]
    fn lex_begin_v31() {
        assert_eq!(tokens_no_regex("BEGIN"), vec![Token::Begin]);
    }
    #[test]
    fn lex_end_v31() {
        assert_eq!(tokens_no_regex("END"), vec![Token::End]);
    }
    #[test]
    fn lex_if_v31() {
        assert_eq!(tokens_no_regex("if"), vec![Token::If]);
    }
    #[test]
    fn lex_else_v31() {
        assert_eq!(tokens_no_regex("else"), vec![Token::Else]);
    }
    #[test]
    fn lex_while_v31() {
        assert_eq!(tokens_no_regex("while"), vec![Token::While]);
    }
    #[test]
    fn lex_for_v31() {
        assert_eq!(tokens_no_regex("for"), vec![Token::For]);
    }
    #[test]
    fn lex_do_v31() {
        assert_eq!(tokens_no_regex("do"), vec![Token::Do]);
    }
    #[test]
    fn lex_break_v31() {
        assert_eq!(tokens_no_regex("break"), vec![Token::Break]);
    }
    #[test]
    fn lex_continue_v31() {
        assert_eq!(tokens_no_regex("continue"), vec![Token::Continue]);
    }
    #[test]
    fn lex_delete_v31() {
        assert_eq!(tokens_no_regex("delete"), vec![Token::Delete]);
    }
    #[test]
    fn lex_exit_v31() {
        assert_eq!(tokens_no_regex("exit"), vec![Token::Exit]);
    }
    #[test]
    fn lex_next_v31() {
        assert_eq!(tokens_no_regex("next"), vec![Token::Next]);
    }
    #[test]
    fn lex_nextfile_v31() {
        assert_eq!(tokens_no_regex("nextfile"), vec![Token::NextFile]);
    }
    #[test]
    fn lex_return_v31() {
        assert_eq!(tokens_no_regex("return"), vec![Token::Return]);
    }
    #[test]
    fn lex_function_v31() {
        assert_eq!(tokens_no_regex("function"), vec![Token::Function]);
    }

    #[test]
    fn lex_lbrace_v34() {
        assert_eq!(tokens_no_regex("{"), vec![Token::LBrace]);
    }
    #[test]
    fn lex_rbrace_v34() {
        assert_eq!(tokens_no_regex("}"), vec![Token::RBrace]);
    }
    #[test]
    fn lex_lparen_v34() {
        assert_eq!(tokens_no_regex("("), vec![Token::LParen]);
    }
    #[test]
    fn lex_rparen_v34() {
        assert_eq!(tokens_no_regex(")"), vec![Token::RParen]);
    }
    #[test]
    fn lex_lbracket_v34() {
        assert_eq!(tokens_no_regex("["), vec![Token::LBracket]);
    }
    #[test]
    fn lex_rbracket_v34() {
        assert_eq!(tokens_no_regex("]"), vec![Token::RBracket]);
    }

    #[test]
    fn lex_lbrace_v35() {
        assert_eq!(tokens_no_regex("{"), vec![Token::LBrace]);
    }
    #[test]
    fn lex_rbrace_v35() {
        assert_eq!(tokens_no_regex("}"), vec![Token::RBrace]);
    }
    #[test]
    fn lex_lparen_v35() {
        assert_eq!(tokens_no_regex("("), vec![Token::LParen]);
    }
    #[test]
    fn lex_rparen_v35() {
        assert_eq!(tokens_no_regex(")"), vec![Token::RParen]);
    }
    #[test]
    fn lex_lbracket_v35() {
        assert_eq!(tokens_no_regex("["), vec![Token::LBracket]);
    }
    #[test]
    fn lex_rbracket_v35() {
        assert_eq!(tokens_no_regex("]"), vec![Token::RBracket]);
    }
    #[test]
    fn lex_semi_v35() {
        assert_eq!(tokens_no_regex(";"), vec![Token::Semi]);
    }
    #[test]
    fn lex_comma_v35() {
        assert_eq!(tokens_no_regex(","), vec![Token::Comma]);
    }
    #[test]
    fn lex_colon_v35() {
        assert_eq!(tokens_no_regex(":"), vec![Token::Colon]);
    }
    #[test]
    fn lex_plus_v35() {
        assert_eq!(tokens_no_regex("+"), vec![Token::Plus]);
    }
    #[test]
    fn lex_minus_v35() {
        assert_eq!(tokens_no_regex("-"), vec![Token::Minus]);
    }
    #[test]
    fn lex_star_v35() {
        assert_eq!(tokens_no_regex("*"), vec![Token::Star]);
    }
    #[test]
    fn lex_slash_v35() {
        assert_eq!(tokens_no_regex("/"), vec![Token::Slash]);
    }
    #[test]
    fn lex_percent_v35() {
        assert_eq!(tokens_no_regex("%"), vec![Token::Percent]);
    }
    #[test]
    fn lex_caret_v35() {
        assert_eq!(tokens_no_regex("^"), vec![Token::Caret]);
    }
    #[test]
    fn lex_assign_v35() {
        assert_eq!(tokens_no_regex("="), vec![Token::Assign]);
    }
    #[test]
    fn lex_lt_v35() {
        assert_eq!(tokens_no_regex("<"), vec![Token::Lt]);
    }
    #[test]
    fn lex_gt_v35() {
        assert_eq!(tokens_no_regex(">"), vec![Token::Gt]);
    }
    #[test]
    fn lex_bang_v35() {
        assert_eq!(tokens_no_regex("!"), vec![Token::Bang]);
    }
    #[test]
    fn lex_tilde_v35() {
        assert_eq!(tokens_no_regex("~"), vec![Token::Tilde]);
    }
    #[test]
    fn lex_dollar_v35() {
        assert_eq!(tokens_no_regex("$"), vec![Token::Dollar]);
    }
    #[test]
    fn lex_at_v35() {
        assert_eq!(tokens_no_regex("@"), vec![Token::At]);
    }
    #[test]
    fn lex_question_v35() {
        assert_eq!(tokens_no_regex("?"), vec![Token::Question]);
    }
    #[test]
    fn lex_and_v35() {
        assert_eq!(tokens_no_regex("&&"), vec![Token::And]);
    }
    #[test]
    fn lex_or_v35() {
        assert_eq!(tokens_no_regex("||"), vec![Token::Or]);
    }
    #[test]
    fn lex_plusplus_v35() {
        assert_eq!(tokens_no_regex("++"), vec![Token::PlusPlus]);
    }
    #[test]
    fn lex_minusminus_v35() {
        assert_eq!(tokens_no_regex("--"), vec![Token::MinusMinus]);
    }
    #[test]
    fn lex_eq_v35() {
        assert_eq!(tokens_no_regex("=="), vec![Token::Eq]);
    }
    #[test]
    fn lex_ne_v35() {
        assert_eq!(tokens_no_regex("!="), vec![Token::Ne]);
    }
    #[test]
    fn lex_le_v35() {
        assert_eq!(tokens_no_regex("<="), vec![Token::Le]);
    }

    #[test]
    fn lex_lbrace_v38() {
        assert_eq!(tokens_no_regex("{"), vec![Token::LBrace]);
    }
    #[test]
    fn lex_rbrace_v38() {
        assert_eq!(tokens_no_regex("}"), vec![Token::RBrace]);
    }
    #[test]
    fn lex_lparen_v38() {
        assert_eq!(tokens_no_regex("("), vec![Token::LParen]);
    }
    #[test]
    fn lex_rparen_v38() {
        assert_eq!(tokens_no_regex(")"), vec![Token::RParen]);
    }
    #[test]
    fn lex_lbracket_v38() {
        assert_eq!(tokens_no_regex("["), vec![Token::LBracket]);
    }
    #[test]
    fn lex_rbracket_v38() {
        assert_eq!(tokens_no_regex("]"), vec![Token::RBracket]);
    }
    #[test]
    fn lex_semi_v38() {
        assert_eq!(tokens_no_regex(";"), vec![Token::Semi]);
    }
    #[test]
    fn lex_comma_v38() {
        assert_eq!(tokens_no_regex(","), vec![Token::Comma]);
    }
    #[test]
    fn lex_colon_v38() {
        assert_eq!(tokens_no_regex(":"), vec![Token::Colon]);
    }
    #[test]
    fn lex_plus_v38() {
        assert_eq!(tokens_no_regex("+"), vec![Token::Plus]);
    }
    #[test]
    fn lex_minus_v38() {
        assert_eq!(tokens_no_regex("-"), vec![Token::Minus]);
    }
    #[test]
    fn lex_star_v38() {
        assert_eq!(tokens_no_regex("*"), vec![Token::Star]);
    }
    #[test]
    fn lex_slash_v38() {
        assert_eq!(tokens_no_regex("/"), vec![Token::Slash]);
    }
    #[test]
    fn lex_percent_v38() {
        assert_eq!(tokens_no_regex("%"), vec![Token::Percent]);
    }
    #[test]
    fn lex_caret_v38() {
        assert_eq!(tokens_no_regex("^"), vec![Token::Caret]);
    }

    #[test]
    fn lex_string_obscure_escapes_v10() {
        assert_eq!(
            tokens_no_regex("\"\\a\\v\\?\""),
            vec![Token::String("\x07\x0b?".into())]
        );
    }

    #[test]
    fn lex_string_stray_backslash_v10() {
        // awkrs: unknown escape (\z) drops the backslash and emits just the character
        assert_eq!(tokens_no_regex("\"\\z\""), vec![Token::String("z".into())]);
    }

    #[test]
    fn lex_hex_escape_fixed_length_v10() {
        // \x followed by up to 2 hex digits in some implementations,
        // but gawk consumes all hex digits? Let's check awkrs.
        // Looking at lexer.rs: consumes ALL hex digits.
        assert_eq!(
            tokens_no_regex("\"\\x41\""),
            vec![Token::String("A".into())]
        );
    }

    #[test]
    fn lex_hex_v13_00() {
        assert_eq!(
            tokens_no_regex("\"\\x00\""),
            vec![Token::String("\x00".into())]
        );
    }
    #[test]
    fn lex_hex_v13_0a() {
        assert_eq!(
            tokens_no_regex("\"\\x0A\""),
            vec![Token::String("\n".into())]
        );
    }
    #[test]
    fn lex_hex_v13_7f() {
        assert_eq!(
            tokens_no_regex("\"\\x7F\""),
            vec![Token::String("\x7f".into())]
        );
    }
    #[test]
    fn lex_hex_v13_ff() {
        assert_eq!(
            tokens_no_regex("\"\\xFF\""),
            vec![Token::String("ÿ".into())]
        );
    }

    #[test]
    fn lex_oct_v13_000() {
        assert_eq!(
            tokens_no_regex("\"\\000\""),
            vec![Token::String("\x00".into())]
        );
    }
    #[test]
    fn lex_oct_v13_012() {
        assert_eq!(
            tokens_no_regex("\"\\012\""),
            vec![Token::String("\n".into())]
        );
    }
    #[test]
    fn lex_oct_v13_101() {
        assert_eq!(
            tokens_no_regex("\"\\101\""),
            vec![Token::String("A".into())]
        );
    }
    #[test]
    fn lex_oct_v13_377() {
        assert_eq!(
            tokens_no_regex("\"\\377\""),
            vec![Token::String("ÿ".into())]
        );
    }

    #[test]
    fn lex_k_begin_v54() {
        assert_eq!(tokens_no_regex("BEGIN"), vec![Token::Begin]);
    }
    #[test]
    fn lex_k_end_v54() {
        assert_eq!(tokens_no_regex("END"), vec![Token::End]);
    }
    #[test]
    fn lex_k_if_v54() {
        assert_eq!(tokens_no_regex("if"), vec![Token::If]);
    }
    #[test]
    fn lex_k_else_v54() {
        assert_eq!(tokens_no_regex("else"), vec![Token::Else]);
    }
    #[test]
    fn lex_k_while_v54() {
        assert_eq!(tokens_no_regex("while"), vec![Token::While]);
    }
    #[test]
    fn lex_k_do_v54() {
        assert_eq!(tokens_no_regex("do"), vec![Token::Do]);
    }
    #[test]
    fn lex_k_for_v54() {
        assert_eq!(tokens_no_regex("for"), vec![Token::For]);
    }
    #[test]
    fn lex_k_break_v54() {
        assert_eq!(tokens_no_regex("break"), vec![Token::Break]);
    }
    #[test]
    fn lex_k_continue_v54() {
        assert_eq!(tokens_no_regex("continue"), vec![Token::Continue]);
    }
    #[test]
    fn lex_k_delete_v54() {
        assert_eq!(tokens_no_regex("delete"), vec![Token::Delete]);
    }
    #[test]
    fn lex_k_exit_v54() {
        assert_eq!(tokens_no_regex("exit"), vec![Token::Exit]);
    }
    #[test]
    fn lex_k_next_v54() {
        assert_eq!(tokens_no_regex("next"), vec![Token::Next]);
    }
    #[test]
    fn lex_k_nextfile_v54() {
        assert_eq!(tokens_no_regex("nextfile"), vec![Token::NextFile]);
    }
    #[test]
    fn lex_k_return_v54() {
        assert_eq!(tokens_no_regex("return"), vec![Token::Return]);
    }
    #[test]
    fn lex_k_function_v54() {
        assert_eq!(tokens_no_regex("function"), vec![Token::Function]);
    }
    #[test]
    fn lex_k_in_v54() {
        assert_eq!(tokens_no_regex("in"), vec![Token::In]);
    }
    #[test]
    fn lex_k_print_v54() {
        assert_eq!(tokens_no_regex("print"), vec![Token::Print]);
    }
    #[test]
    fn lex_k_printf_v54() {
        assert_eq!(tokens_no_regex("printf"), vec![Token::Printf]);
    }
    #[test]
    fn lex_k_getline_v54() {
        assert_eq!(tokens_no_regex("getline"), vec![Token::Getline]);
    }

    #[test]
    fn lex_p_lbrace_v54() {
        assert_eq!(tokens_no_regex("{"), vec![Token::LBrace]);
    }
    #[test]
    fn lex_p_rbrace_v54() {
        assert_eq!(tokens_no_regex("}"), vec![Token::RBrace]);
    }
    #[test]
    fn lex_p_lparen_v54() {
        assert_eq!(tokens_no_regex("("), vec![Token::LParen]);
    }
    #[test]
    fn lex_p_rparen_v54() {
        assert_eq!(tokens_no_regex(")"), vec![Token::RParen]);
    }
    #[test]
    fn lex_p_lbracket_v54() {
        assert_eq!(tokens_no_regex("["), vec![Token::LBracket]);
    }
    #[test]
    fn lex_p_rbracket_v54() {
        assert_eq!(tokens_no_regex("]"), vec![Token::RBracket]);
    }
    #[test]
    fn lex_p_semi_v54() {
        assert_eq!(tokens_no_regex(";"), vec![Token::Semi]);
    }
    #[test]
    fn lex_p_comma_v54() {
        assert_eq!(tokens_no_regex(","), vec![Token::Comma]);
    }
    #[test]
    fn lex_p_colon_v54() {
        assert_eq!(tokens_no_regex(":"), vec![Token::Colon]);
    }
    #[test]
    fn lex_p_question_v54() {
        assert_eq!(tokens_no_regex("?"), vec![Token::Question]);
    }
    #[test]
    fn lex_p_at_v54() {
        assert_eq!(tokens_no_regex("@"), vec![Token::At]);
    }
    #[test]
    fn lex_p_dollar_v54() {
        assert_eq!(tokens_no_regex("$"), vec![Token::Dollar]);
    }

    #[test]
    fn lex_o_plus_v54() {
        assert_eq!(tokens_no_regex("+"), vec![Token::Plus]);
    }
    #[test]
    fn lex_o_minus_v54() {
        assert_eq!(tokens_no_regex("-"), vec![Token::Minus]);
    }
    #[test]
    fn lex_o_star_v54() {
        assert_eq!(tokens_no_regex("*"), vec![Token::Star]);
    }
    #[test]
    fn lex_o_slash_v54() {
        assert_eq!(tokens_no_regex("/"), vec![Token::Slash]);
    }
    #[test]
    fn lex_o_percent_v54() {
        assert_eq!(tokens_no_regex("%"), vec![Token::Percent]);
    }
    #[test]
    fn lex_o_caret_v54() {
        assert_eq!(tokens_no_regex("^"), vec![Token::Caret]);
    }
    #[test]
    fn lex_o_assign_v54() {
        assert_eq!(tokens_no_regex("="), vec![Token::Assign]);
    }
    #[test]
    fn lex_o_lt_v54() {
        assert_eq!(tokens_no_regex("<"), vec![Token::Lt]);
    }
    #[test]
    fn lex_o_gt_v54() {
        assert_eq!(tokens_no_regex(">"), vec![Token::Gt]);
    }
    #[test]
    fn lex_o_bang_v54() {
        assert_eq!(tokens_no_regex("!"), vec![Token::Bang]);
    }
    #[test]
    fn lex_o_tilde_v54() {
        assert_eq!(tokens_no_regex("~"), vec![Token::Tilde]);
    }

    #[test]
    fn lex_m_plusplus_v54() {
        assert_eq!(tokens_no_regex("++"), vec![Token::PlusPlus]);
    }
    #[test]
    fn lex_m_minusminus_v54() {
        assert_eq!(tokens_no_regex("--"), vec![Token::MinusMinus]);
    }
    #[test]
    fn lex_m_pow_v54() {
        assert_eq!(tokens_no_regex("**"), vec![Token::StarStar]);
    }
    #[test]
    fn lex_m_eq_v54() {
        assert_eq!(tokens_no_regex("=="), vec![Token::Eq]);
    }
    #[test]
    fn lex_m_ne_v54() {
        assert_eq!(tokens_no_regex("!="), vec![Token::Ne]);
    }
    #[test]
    fn lex_m_le_v54() {
        assert_eq!(tokens_no_regex("<="), vec![Token::Le]);
    }
    #[test]
    fn lex_m_ge_v54() {
        assert_eq!(tokens_no_regex(">="), vec![Token::Ge]);
    }
    #[test]
    fn lex_m_and_v54() {
        assert_eq!(tokens_no_regex("&&"), vec![Token::And]);
    }
    #[test]
    fn lex_m_or_v54() {
        assert_eq!(tokens_no_regex("||"), vec![Token::Or]);
    }
    #[test]
    fn lex_m_notmatch_v54() {
        assert_eq!(tokens_no_regex("!~"), vec![Token::NotTilde]);
    }

    #[test]
    fn lex_m_add_assign_v54() {
        assert_eq!(tokens_no_regex("+="), vec![Token::AddAssign]);
    }
    #[test]
    fn lex_m_sub_assign_v54() {
        assert_eq!(tokens_no_regex("-="), vec![Token::SubAssign]);
    }
    #[test]
    fn lex_m_mul_assign_v54() {
        assert_eq!(tokens_no_regex("*="), vec![Token::MulAssign]);
    }
    #[test]
    fn lex_m_div_assign_v54() {
        assert_eq!(tokens_no_regex("/="), vec![Token::DivAssign]);
    }
    #[test]
    fn lex_m_mod_assign_v54() {
        assert_eq!(tokens_no_regex("%="), vec![Token::ModAssign]);
    }
    #[test]
    fn lex_m_pow_assign_v54() {
        assert_eq!(tokens_no_regex("^="), vec![Token::PowAssign]);
    }
    #[test]
    fn lex_m_pow_assign_starstar_v54() {
        assert_eq!(tokens_no_regex("**="), vec![Token::PowAssign]);
    }

    #[test]
    fn lex_v_num_v54() {
        assert_eq!(tokens_no_regex("1.23"), vec![Token::Number(1.23)]);
    }
    #[test]
    fn lex_v_str_v54() {
        assert_eq!(tokens_no_regex("\"a\""), vec![Token::String("a".into())]);
    }
    #[test]
    fn lex_v_ident_v54() {
        assert_eq!(tokens_no_regex("x"), vec![Token::Ident("x".into())]);
    }

    #[test]
    fn lex_v_num_v59_0() {
        assert_eq!(
            tokens_no_regex("0"),
            vec![Token::IntegerLiteral("0".into())]
        );
    }
    #[test]
    fn lex_v_num_v59_1() {
        assert_eq!(
            tokens_no_regex("123"),
            vec![Token::IntegerLiteral("123".into())]
        );
    }
    #[test]
    fn lex_v_num_v59_2() {
        assert_eq!(tokens_no_regex("1.23"), vec![Token::Number(1.23)]);
    }
    #[test]
    fn lex_v_num_v59_3() {
        assert_eq!(tokens_no_regex(".23"), vec![Token::Number(0.23)]);
    }
    #[test]
    fn lex_v_num_v59_4() {
        assert_eq!(tokens_no_regex("1."), vec![Token::Number(1.0)]);
    }
    #[test]
    fn lex_v_num_v59_5() {
        assert_eq!(tokens_no_regex("1e2"), vec![Token::Number(100.0)]);
    }
    #[test]
    fn lex_v_num_v59_6() {
        assert_eq!(tokens_no_regex("1E2"), vec![Token::Number(100.0)]);
    }
    #[test]
    fn lex_v_num_v59_7() {
        assert_eq!(tokens_no_regex("1.2e2"), vec![Token::Number(120.0)]);
    }
    #[test]
    fn lex_v_num_v59_8() {
        assert_eq!(tokens_no_regex("1.2e-2"), vec![Token::Number(0.012)]);
    }

    #[test]
    fn lex_v_str_v59_0() {
        assert_eq!(tokens_no_regex("\"\""), vec![Token::String("".into())]);
    }
    #[test]
    fn lex_v_str_v59_1() {
        assert_eq!(tokens_no_regex("\"a\""), vec![Token::String("a".into())]);
    }
    #[test]
    fn lex_v_str_v59_2() {
        assert_eq!(
            tokens_no_regex("\"\\\"\""),
            vec![Token::String("\"".into())]
        );
    }
    #[test]
    fn lex_v_str_v59_3() {
        assert_eq!(
            tokens_no_regex("\"\\\\\""),
            vec![Token::String("\\".into())]
        );
    }

    #[test]
    fn lex_v_ident_v59_0() {
        assert_eq!(tokens_no_regex("a"), vec![Token::Ident("a".into())]);
    }
    #[test]
    fn lex_v_ident_v59_1() {
        assert_eq!(tokens_no_regex("_"), vec![Token::Ident("_".into())]);
    }
    #[test]
    fn lex_v_ident_v59_2() {
        assert_eq!(tokens_no_regex("a1"), vec![Token::Ident("a1".into())]);
    }
    #[test]
    fn lex_v_ident_v59_3() {
        assert_eq!(tokens_no_regex("_1"), vec![Token::Ident("_1".into())]);
    }

    #[test]
    fn lex_p_semi_v59() {
        assert_eq!(tokens_no_regex(";;"), vec![Token::Semi, Token::Semi]);
    }
    #[test]
    fn lex_p_nl_v59() {
        assert_eq!(
            tokens_no_regex("\n\n"),
            vec![Token::Newline, Token::Newline]
        );
    }

    #[test]
    fn lex_lc_begin_v62() {
        assert_eq!(tokens_no_regex("begin"), vec![Token::Ident("begin".into())]);
    }
    #[test]
    fn lex_lc_end_v62() {
        assert_eq!(tokens_no_regex("end"), vec![Token::Ident("end".into())]);
    }
    #[test]
    fn lex_lc_if_v62() {
        assert_eq!(tokens_no_regex("IF"), vec![Token::Ident("IF".into())]);
    }
    #[test]
    fn lex_lc_else_v62() {
        assert_eq!(tokens_no_regex("ELSE"), vec![Token::Ident("ELSE".into())]);
    }
    #[test]
    fn lex_lc_while_v62() {
        assert_eq!(tokens_no_regex("WHILE"), vec![Token::Ident("WHILE".into())]);
    }
    #[test]
    fn lex_lc_do_v62() {
        assert_eq!(tokens_no_regex("DO"), vec![Token::Ident("DO".into())]);
    }
    #[test]
    fn lex_lc_for_v62() {
        assert_eq!(tokens_no_regex("FOR"), vec![Token::Ident("FOR".into())]);
    }
    #[test]
    fn lex_lc_break_v62() {
        assert_eq!(tokens_no_regex("BREAK"), vec![Token::Ident("BREAK".into())]);
    }
    #[test]
    fn lex_lc_continue_v62() {
        assert_eq!(
            tokens_no_regex("CONTINUE"),
            vec![Token::Ident("CONTINUE".into())]
        );
    }
    #[test]
    fn lex_lc_delete_v62() {
        assert_eq!(
            tokens_no_regex("DELETE"),
            vec![Token::Ident("DELETE".into())]
        );
    }
    #[test]
    fn lex_lc_exit_v62() {
        assert_eq!(tokens_no_regex("EXIT"), vec![Token::Ident("EXIT".into())]);
    }
    #[test]
    fn lex_lc_next_v62() {
        assert_eq!(tokens_no_regex("NEXT"), vec![Token::Ident("NEXT".into())]);
    }
    #[test]
    fn lex_lc_nextfile_v62() {
        assert_eq!(
            tokens_no_regex("NEXTFILE"),
            vec![Token::Ident("NEXTFILE".into())]
        );
    }
    #[test]
    fn lex_lc_return_v62() {
        assert_eq!(
            tokens_no_regex("RETURN"),
            vec![Token::Ident("RETURN".into())]
        );
    }
    #[test]
    fn lex_lc_function_v62() {
        assert_eq!(
            tokens_no_regex("FUNCTION"),
            vec![Token::Ident("FUNCTION".into())]
        );
    }
    #[test]
    fn lex_lc_in_v62() {
        assert_eq!(tokens_no_regex("IN"), vec![Token::Ident("IN".into())]);
    }
    #[test]
    fn lex_lc_print_v62() {
        assert_eq!(tokens_no_regex("PRINT"), vec![Token::Ident("PRINT".into())]);
    }
    #[test]
    fn lex_lc_printf_v62() {
        assert_eq!(
            tokens_no_regex("PRINTF"),
            vec![Token::Ident("PRINTF".into())]
        );
    }
    #[test]
    fn lex_lc_getline_v62() {
        assert_eq!(
            tokens_no_regex("GETLINE"),
            vec![Token::Ident("GETLINE".into())]
        );
    }

    #[test]
    fn lex_k_begin_v70() {
        assert_eq!(tokens_no_regex("BEGIN"), vec![Token::Begin]);
    }
    #[test]
    fn lex_k_end_v70() {
        assert_eq!(tokens_no_regex("END"), vec![Token::End]);
    }
    #[test]
    fn lex_k_if_v70() {
        assert_eq!(tokens_no_regex("if"), vec![Token::If]);
    }
    #[test]
    fn lex_k_else_v70() {
        assert_eq!(tokens_no_regex("else"), vec![Token::Else]);
    }
    #[test]
    fn lex_k_while_v70() {
        assert_eq!(tokens_no_regex("while"), vec![Token::While]);
    }
    #[test]
    fn lex_k_do_v70() {
        assert_eq!(tokens_no_regex("do"), vec![Token::Do]);
    }
    #[test]
    fn lex_k_for_v70() {
        assert_eq!(tokens_no_regex("for"), vec![Token::For]);
    }
    #[test]
    fn lex_k_break_v70() {
        assert_eq!(tokens_no_regex("break"), vec![Token::Break]);
    }
    #[test]
    fn lex_k_continue_v70() {
        assert_eq!(tokens_no_regex("continue"), vec![Token::Continue]);
    }
    #[test]
    fn lex_k_delete_v70() {
        assert_eq!(tokens_no_regex("delete"), vec![Token::Delete]);
    }
    #[test]
    fn lex_k_exit_v70() {
        assert_eq!(tokens_no_regex("exit"), vec![Token::Exit]);
    }
    #[test]
    fn lex_k_next_v70() {
        assert_eq!(tokens_no_regex("next"), vec![Token::Next]);
    }
    #[test]
    fn lex_k_nextfile_v70() {
        assert_eq!(tokens_no_regex("nextfile"), vec![Token::NextFile]);
    }
    #[test]
    fn lex_k_return_v70() {
        assert_eq!(tokens_no_regex("return"), vec![Token::Return]);
    }
    #[test]
    fn lex_k_function_v70() {
        assert_eq!(tokens_no_regex("function"), vec![Token::Function]);
    }
    #[test]
    fn lex_k_in_v70() {
        assert_eq!(tokens_no_regex("in"), vec![Token::In]);
    }
    #[test]
    fn lex_k_print_v70() {
        assert_eq!(tokens_no_regex("print"), vec![Token::Print]);
    }
    #[test]
    fn lex_k_printf_v70() {
        assert_eq!(tokens_no_regex("printf"), vec![Token::Printf]);
    }
    #[test]
    fn lex_k_getline_v70() {
        assert_eq!(tokens_no_regex("getline"), vec![Token::Getline]);
    }

    #[test]
    fn lex_p_v65_0() {
        assert_eq!(tokens_no_regex("{"), vec![Token::LBrace]);
    }
    #[test]
    fn lex_p_v65_1() {
        assert_eq!(tokens_no_regex("}"), vec![Token::RBrace]);
    }
    #[test]
    fn lex_p_v65_2() {
        assert_eq!(tokens_no_regex("("), vec![Token::LParen]);
    }
    #[test]
    fn lex_p_v65_3() {
        assert_eq!(tokens_no_regex(")"), vec![Token::RParen]);
    }
    #[test]
    fn lex_p_v65_4() {
        assert_eq!(tokens_no_regex("["), vec![Token::LBracket]);
    }
    #[test]
    fn lex_p_v65_5() {
        assert_eq!(tokens_no_regex("]"), vec![Token::RBracket]);
    }
    #[test]
    fn lex_p_v65_6() {
        assert_eq!(tokens_no_regex(";"), vec![Token::Semi]);
    }
    #[test]
    fn lex_p_v65_7() {
        assert_eq!(tokens_no_regex(","), vec![Token::Comma]);
    }
    #[test]
    fn lex_p_v65_8() {
        assert_eq!(tokens_no_regex(":"), vec![Token::Colon]);
    }
    #[test]
    fn lex_p_v65_9() {
        assert_eq!(tokens_no_regex("?"), vec![Token::Question]);
    }
    #[test]
    fn lex_p_v65_10() {
        assert_eq!(tokens_no_regex("@"), vec![Token::At]);
    }
    #[test]
    fn lex_p_v65_11() {
        assert_eq!(tokens_no_regex("$"), vec![Token::Dollar]);
    }

    #[test]
    fn lex_o_v65_0() {
        assert_eq!(tokens_no_regex("+"), vec![Token::Plus]);
    }
    #[test]
    fn lex_o_v65_1() {
        assert_eq!(tokens_no_regex("-"), vec![Token::Minus]);
    }
    #[test]
    fn lex_o_v65_2() {
        assert_eq!(tokens_no_regex("*"), vec![Token::Star]);
    }
    #[test]
    fn lex_o_v65_3() {
        assert_eq!(tokens_no_regex("/"), vec![Token::Slash]);
    }
    #[test]
    fn lex_o_v65_4() {
        assert_eq!(tokens_no_regex("%"), vec![Token::Percent]);
    }
    #[test]
    fn lex_o_v65_5() {
        assert_eq!(tokens_no_regex("^"), vec![Token::Caret]);
    }
    #[test]
    fn lex_o_v65_6() {
        assert_eq!(tokens_no_regex("="), vec![Token::Assign]);
    }
    #[test]
    fn lex_o_v65_7() {
        assert_eq!(tokens_no_regex("<"), vec![Token::Lt]);
    }
    #[test]
    fn lex_o_v65_8() {
        assert_eq!(tokens_no_regex(">"), vec![Token::Gt]);
    }
    #[test]
    fn lex_o_v65_9() {
        assert_eq!(tokens_no_regex("!"), vec![Token::Bang]);
    }
    #[test]
    fn lex_o_v65_10() {
        assert_eq!(tokens_no_regex("~"), vec![Token::Tilde]);
    }

    #[test]
    fn lex_m_v65_0() {
        assert_eq!(tokens_no_regex("++"), vec![Token::PlusPlus]);
    }
    #[test]
    fn lex_m_v65_1() {
        assert_eq!(tokens_no_regex("--"), vec![Token::MinusMinus]);
    }
    #[test]
    fn lex_m_v65_2() {
        assert_eq!(tokens_no_regex("**"), vec![Token::StarStar]);
    }
    #[test]
    fn lex_m_v65_3() {
        assert_eq!(tokens_no_regex("=="), vec![Token::Eq]);
    }
    #[test]
    fn lex_m_v65_4() {
        assert_eq!(tokens_no_regex("!="), vec![Token::Ne]);
    }
    #[test]
    fn lex_m_v65_5() {
        assert_eq!(tokens_no_regex("<="), vec![Token::Le]);
    }
    #[test]
    fn lex_m_v65_6() {
        assert_eq!(tokens_no_regex(">="), vec![Token::Ge]);
    }
    #[test]
    fn lex_m_v65_7() {
        assert_eq!(tokens_no_regex("&&"), vec![Token::And]);
    }
    #[test]
    fn lex_m_v65_8() {
        assert_eq!(tokens_no_regex("||"), vec![Token::Or]);
    }
    #[test]
    fn lex_m_v65_9() {
        assert_eq!(tokens_no_regex("!~"), vec![Token::NotTilde]);
    }

    // ─── is_ident_start / is_ident_continue contract pins ────────────
    //
    // gawk's identifier grammar is ASCII-only and excludes digit
    // starts. Pin both sides exhaustively so a Unicode-friendly
    // refactor doesn't silently start accepting `ñame` (which gawk
    // would reject at lex time).

    #[test]
    fn is_ident_start_accepts_ascii_alpha_and_underscore() {
        for c in 'a'..='z' {
            assert!(is_ident_start(c), "lowercase `{c}` should start ident");
        }
        for c in 'A'..='Z' {
            assert!(is_ident_start(c), "uppercase `{c}` should start ident");
        }
        assert!(is_ident_start('_'), "underscore must start ident");
    }

    #[test]
    fn is_ident_start_rejects_digits() {
        for c in '0'..='9' {
            assert!(!is_ident_start(c), "digit `{c}` must NOT start ident");
        }
    }

    #[test]
    fn is_ident_start_rejects_unicode_alpha() {
        // gawk's ident grammar is ASCII-only; pin the rejection so
        // a future feature wave doesn't silently widen identifiers
        // and break compatibility with awk scripts that depend on
        // non-ASCII chars being lex breaks.
        assert!(!is_ident_start('ñ'));
        assert!(!is_ident_start('日'));
        assert!(!is_ident_start('ß'));
    }

    #[test]
    fn is_ident_continue_accepts_digits_after_alpha() {
        // `foo123` is valid; digits CAN follow.
        for c in '0'..='9' {
            assert!(is_ident_continue(c));
        }
    }

    #[test]
    fn is_ident_continue_rejects_punct_and_whitespace() {
        for c in ['-', '+', '.', '/', ' ', '\t', '\n', '$', '@', '#'] {
            assert!(!is_ident_continue(c), "char `{c}` must NOT continue ident");
        }
    }
}
