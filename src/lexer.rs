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
            let slice = &self.input[start..self.pos];
            if !had_dot {
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
                    Token::StarStar
                } else if self.peek() == Some('=') {
                    self.bump();
                    Token::MulAssign
                } else {
                    Token::Star
                }
            }
            '^' => Token::Caret,
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
        assert_eq!(
            tokens_no_regex("# whole line comment\nx"),
            vec![Token::Ident("x".into())]
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
    fn lex_scientific_notation_splits_integer_and_ident() {
        // Lexer does not fold `e` exponents into one float token; callers rely on parse layer.
        assert_eq!(
            tokens_no_regex("1e3"),
            vec![Token::IntegerLiteral("1".into()), Token::Ident("e3".into())]
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
    fn lex_caret_followed_by_assign_is_two_tokens() {
        assert_eq!(tokens_no_regex("^="), vec![Token::Caret, Token::Assign]);
    }

    #[test]
    fn lex_mod_assign_token() {
        assert_eq!(tokens_no_regex("%="), vec![Token::ModAssign]);
    }

    #[test]
    fn lex_single_gt_not_append() {
        assert_eq!(tokens_no_regex(">"), vec![Token::Gt]);
    }
}
