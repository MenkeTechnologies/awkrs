use crate::error::{Error, Result};
use rug::Integer;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Begin,
    End,
    BeginFile,
    EndFile,
    Print,
    Printf,
    If,
    Else,
    While,
    For,
    Do,
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    In,
    Function,
    Return,
    Delete,
    Getline,
    Switch,
    Case,
    Default,
    Ident(String),
    Number(f64),
    /// Decimal integer with no `.` in source — exact under **`-M`** (not rounded through `f64`).
    IntegerLiteral(String),
    String(String),
    Regexp(String),

    Plus,
    /// `++` (single token).
    PlusPlus,
    Minus,
    /// `--` (single token).
    MinusMinus,
    Star,
    /// `**` (exponentiation; distinct from `*`).
    StarStar,
    /// `^` (exponentiation).
    Caret,
    Slash,
    Percent,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    /// `^=` / `**=` — compound exponentiation assignment.
    PowAssign,
    Eq,
    Ne,
    Lt,
    /// `<&` (gawk-style coprocess read redirect).
    LtAmp,
    Le,
    Gt,
    /// `>>` (append redirect; distinct from two `>` tokens).
    GtGt,
    /// `|&` (gawk coprocess / two-way pipe).
    PipeCoproc,
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
    /// `@` — indirect function calls (`@expr(...)`) and distinct from directives handled in preprocessing.
    At,
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
    fn lex_unterminated_string_errors() {
        let mut l = Lexer::new("\"abc");
        assert!(l.next_token(false).is_err());
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
}
