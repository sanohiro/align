//! Lexing: input bytes -> token stream (`docs/impl/02-frontend.md` §1).
//!
//! Statement termination is **Go style** (`draft.md` §4): a newline is an implicit
//! terminator ([`TokKind::End`]). Blocks use `{}`; indentation is insignificant (not
//! Python). A line starting with `.`/a binary operator continues the previous line
//! (multi-line method chains), so no `End` is inserted there.
//!
//! M1 scope: keywords `fn`/`return`/`mut`/`if`/`else`/`true`/`false`, identifiers,
//! integers, `:=` `=` `->`, separators, arithmetic, comparison and logical operators.

use align_diag::Diagnostics;
use align_span::{FileId, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum TokKind {
    // Literals / identifiers
    Int(i128),
    /// Floating-point literal (e.g. `3.14`, `1e9`); the width is fixed by context.
    Float(f64),
    /// Character literal (a Unicode scalar value), e.g. `'a'`, `'\n'`.
    Char(u32),
    /// String literal (its decoded contents), e.g. `"hi\n"`.
    Str(String),
    Ident(String),
    // Keywords
    Fn,
    Return,
    Mut,
    Pub,
    Module,
    Import,
    If,
    Else,
    True,
    False,
    Arena,
    // Symbols / operators
    ColonEq, // :=
    Eq,      // =
    Arrow,   // ->
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,    // ==
    NotEq,   // !=
    Lt,      // <
    Le,      // <=
    Gt,      // >
    Ge,      // >=
    AndAnd,  // &&
    OrOr,    // ||
    Bang,    // !
    Question, // ?
    /// Statement terminator (implicit `;` from a newline, or explicit `;`).
    End,
    Eof,
}

impl TokKind {
    /// Whether this token, at end of line, can terminate a statement (implicit `End`).
    fn can_end_stmt(&self) -> bool {
        matches!(
            self,
            TokKind::Int(_)
                | TokKind::Float(_)
                | TokKind::Char(_)
                | TokKind::Str(_)
                | TokKind::Ident(_)
                | TokKind::Return
                | TokKind::True
                | TokKind::False
                | TokKind::RParen
                | TokKind::RBrace
                | TokKind::RBracket
                | TokKind::Question
        )
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokKind,
    pub span: Span,
}

struct Lexer<'a> {
    file: FileId,
    src: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
}

/// Tokenize `source`. The stream always ends with [`TokKind::Eof`].
pub fn tokenize(file: FileId, source: &str, diags: &mut Diagnostics) -> Vec<Token> {
    let mut lx = Lexer {
        file,
        src: source.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
    };
    lx.run(diags);
    lx.tokens
}

impl<'a> Lexer<'a> {
    fn span(&self, lo: usize, hi: usize) -> Span {
        Span::new(self.file, lo as u32, hi as u32)
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn run(&mut self, diags: &mut Diagnostics) {
        loop {
            self.skip_inline_ws_and_comments();
            match self.peek() {
                None => break,
                Some(b'\n') => {
                    self.pos += 1;
                    self.maybe_insert_end();
                }
                Some(_) => self.lex_token(diags),
            }
        }
        self.maybe_insert_end();
        let at = self.pos;
        self.tokens.push(Token {
            kind: TokKind::Eof,
            span: self.span(at, at),
        });
    }

    /// On a newline, insert an implicit `End` if the previous token can terminate a
    /// statement, unless the next significant byte continues the line (`.`/binary op).
    fn maybe_insert_end(&mut self) {
        let prev_ends = self
            .tokens
            .last()
            .map(|t| t.kind.can_end_stmt())
            .unwrap_or(false);
        if !prev_ends {
            return;
        }
        if self.next_significant_continues_line() {
            return;
        }
        let at = self.pos;
        self.tokens.push(Token {
            kind: TokKind::End,
            span: self.span(at, at),
        });
    }

    /// Whether the next significant byte (after whitespace/comments/newlines) starts a
    /// line continuation: `.` or a binary operator.
    fn next_significant_continues_line(&self) -> bool {
        let mut i = self.pos;
        loop {
            match self.src.get(i).copied() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => i += 1,
                Some(b'.') | Some(b'+') | Some(b'*') | Some(b'/') | Some(b'%') => return true,
                Some(b'<') | Some(b'>') | Some(b'=') | Some(b'&') | Some(b'|') => return true,
                // '-' is also unary, but at line start treat it as a binary continuation.
                Some(b'-') => return self.src.get(i + 1).copied() != Some(b'>'),
                _ => return false,
            }
        }
    }

    fn skip_inline_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') => self.pos += 1,
                Some(b'/') if self.src.get(self.pos + 1) == Some(&b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_token(&mut self, diags: &mut Diagnostics) {
        let start = self.pos;
        let c = self.peek().unwrap();
        match c {
            b'0'..=b'9' => self.lex_number(start, diags),
            b'\'' => self.lex_char(start, diags),
            b'"' => self.lex_string(start, diags),
            c if is_ident_start(c) => self.lex_ident(start),
            _ => self.lex_symbol(start, diags),
        }
    }

    /// Lex an integer or a floating-point literal. A `.` is only consumed as a decimal
    /// point when a digit follows (so `p.x` and `1.method` keep the `.` as a separate
    /// token); an `e`/`E` exponent also makes the literal a float.
    fn lex_number(&mut self, start: usize, diags: &mut Diagnostics) {
        self.eat_digits();
        let mut is_float = false;
        if self.peek() == Some(b'.') && self.src.get(self.pos + 1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.pos += 1; // '.'
            self.eat_digits();
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            self.eat_digits();
        }

        let text: String = std::str::from_utf8(&self.src[start..self.pos])
            .unwrap()
            .chars()
            .filter(|c| *c != '_')
            .collect();
        if is_float {
            match text.parse::<f64>() {
                Ok(v) => self.push(TokKind::Float(v), start),
                Err(_) => {
                    diags.error(format!("invalid float literal: '{text}'"), self.span(start, self.pos));
                }
            }
        } else {
            match text.parse::<i128>() {
                Ok(v) => self.push(TokKind::Int(v), start),
                Err(_) => {
                    diags.error(format!("integer literal out of range: '{text}'"), self.span(start, self.pos));
                }
            }
        }
    }

    fn eat_digits(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Lex a `"..."` string literal (same escapes as char literals).
    fn lex_string(&mut self, start: usize, diags: &mut Diagnostics) {
        self.pos += 1; // opening quote
        let mut s = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    diags.error("unterminated string literal".to_string(), self.span(start, self.pos));
                    break;
                }
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let e = self.peek();
                    self.pos += 1;
                    match e {
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'0') => s.push('\0'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'"') => s.push('"'),
                        other => {
                            diags.error(
                                format!("unknown string escape: '\\{}'", other.map(|b| b as char).unwrap_or('?')),
                                self.span(start, self.pos),
                            );
                        }
                    }
                }
                Some(_) => {
                    let rest = std::str::from_utf8(&self.src[self.pos..]).unwrap_or("\u{FFFD}");
                    let c = rest.chars().next().unwrap_or('\u{FFFD}');
                    self.pos += c.len_utf8();
                    s.push(c);
                }
            }
        }
        self.push(TokKind::Str(s), start);
    }

    /// Lex a `'c'` character literal (one Unicode scalar; supports the common escapes).
    fn lex_char(&mut self, start: usize, diags: &mut Diagnostics) {
        self.pos += 1; // opening quote
        let ch = match self.peek() {
            Some(b'\\') => {
                self.pos += 1;
                let e = self.peek();
                self.pos += 1;
                match e {
                    Some(b'n') => '\n',
                    Some(b't') => '\t',
                    Some(b'r') => '\r',
                    Some(b'0') => '\0',
                    Some(b'\\') => '\\',
                    Some(b'\'') => '\'',
                    other => {
                        diags.error(
                            format!("unknown character escape: '\\{}'", other.map(|b| b as char).unwrap_or('?')),
                            self.span(start, self.pos),
                        );
                        '\u{FFFD}'
                    }
                }
            }
            Some(_) => {
                // Decode one UTF-8 scalar from the remaining bytes.
                let rest = std::str::from_utf8(&self.src[self.pos..]).unwrap_or("\u{FFFD}");
                let c = rest.chars().next().unwrap_or('\u{FFFD}');
                self.pos += c.len_utf8();
                c
            }
            None => {
                diags.error("unterminated character literal".to_string(), self.span(start, self.pos));
                return;
            }
        };
        if self.peek() == Some(b'\'') {
            self.pos += 1;
        } else {
            diags.error(
                "character literal must contain exactly one character".to_string(),
                self.span(start, self.pos),
            );
        }
        self.push(TokKind::Char(ch as u32), start);
    }

    fn lex_ident(&mut self, start: usize) {
        while let Some(c) = self.peek() {
            if is_ident_continue(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        let kind = match text {
            "fn" => TokKind::Fn,
            "return" => TokKind::Return,
            "mut" => TokKind::Mut,
            "pub" => TokKind::Pub,
            "module" => TokKind::Module,
            "import" => TokKind::Import,
            "if" => TokKind::If,
            "else" => TokKind::Else,
            "true" => TokKind::True,
            "false" => TokKind::False,
            "arena" => TokKind::Arena,
            _ => TokKind::Ident(text.to_string()),
        };
        self.push(kind, start);
    }

    fn lex_symbol(&mut self, start: usize, diags: &mut Diagnostics) {
        let c = self.peek().unwrap();
        let two = self.src.get(self.pos + 1).copied();
        let (kind, len) = match (c, two) {
            (b':', Some(b'=')) => (TokKind::ColonEq, 2),
            (b'-', Some(b'>')) => (TokKind::Arrow, 2),
            (b'=', Some(b'=')) => (TokKind::EqEq, 2),
            (b'!', Some(b'=')) => (TokKind::NotEq, 2),
            (b'<', Some(b'=')) => (TokKind::Le, 2),
            (b'>', Some(b'=')) => (TokKind::Ge, 2),
            (b'&', Some(b'&')) => (TokKind::AndAnd, 2),
            (b'|', Some(b'|')) => (TokKind::OrOr, 2),
            (b'=', _) => (TokKind::Eq, 1),
            (b'<', _) => (TokKind::Lt, 1),
            (b'>', _) => (TokKind::Gt, 1),
            (b'!', _) => (TokKind::Bang, 1),
            (b'(', _) => (TokKind::LParen, 1),
            (b')', _) => (TokKind::RParen, 1),
            (b'{', _) => (TokKind::LBrace, 1),
            (b'}', _) => (TokKind::RBrace, 1),
            (b'[', _) => (TokKind::LBracket, 1),
            (b']', _) => (TokKind::RBracket, 1),
            (b',', _) => (TokKind::Comma, 1),
            (b':', _) => (TokKind::Colon, 1),
            (b'.', _) => (TokKind::Dot, 1),
            (b'+', _) => (TokKind::Plus, 1),
            (b'-', _) => (TokKind::Minus, 1),
            (b'*', _) => (TokKind::Star, 1),
            (b'/', _) => (TokKind::Slash, 1),
            (b'%', _) => (TokKind::Percent, 1),
            (b'?', _) => (TokKind::Question, 1),
            (b';', _) => (TokKind::End, 1),
            _ => {
                self.pos += 1;
                diags.error(
                    format!("unexpected character: '{}'", c as char),
                    self.span(start, self.pos),
                );
                return;
            }
        };
        self.pos += len;
        self.push(kind, start);
    }

    fn push(&mut self, kind: TokKind, start: usize) {
        let span = self.span(start, self.pos);
        self.tokens.push(Token { kind, span });
    }
}

fn is_ident_start(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphabetic()
}

fn is_ident_continue(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind> {
        let mut d = Diagnostics::new();
        tokenize(0, src, &mut d)
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn m0_program() {
        let src = "fn main() -> i32 {\n  x := 1\n  return x\n}\n";
        let ks = kinds(src);
        assert_eq!(
            ks,
            vec![
                TokKind::Fn,
                TokKind::Ident("main".into()),
                TokKind::LParen,
                TokKind::RParen,
                TokKind::Arrow,
                TokKind::Ident("i32".into()),
                TokKind::LBrace,
                TokKind::Ident("x".into()),
                TokKind::ColonEq,
                TokKind::Int(1),
                TokKind::End,
                TokKind::Return,
                TokKind::Ident("x".into()),
                TokKind::End,
                TokKind::RBrace,
                TokKind::End,
                TokKind::Eof,
            ]
        );
    }

    #[test]
    fn comparison_and_bool() {
        let ks = kinds("if n <= 2 { true }\n");
        assert_eq!(
            ks,
            vec![
                TokKind::If,
                TokKind::Ident("n".into()),
                TokKind::Le,
                TokKind::Int(2),
                TokKind::LBrace,
                TokKind::True,
                TokKind::RBrace,
                TokKind::End,
                TokKind::Eof,
            ]
        );
    }

    #[test]
    fn float_and_char_literals() {
        // `1.5`, exponent, and a char with an escape; `p.x` keeps `.` as a separate token.
        assert_eq!(
            kinds("3.14\n"),
            vec![TokKind::Float(3.14), TokKind::End, TokKind::Eof]
        );
        assert_eq!(
            kinds("1e3\n"),
            vec![TokKind::Float(1000.0), TokKind::End, TokKind::Eof]
        );
        assert_eq!(
            kinds("p.x"),
            vec![
                TokKind::Ident("p".into()),
                TokKind::Dot,
                TokKind::Ident("x".into()),
                TokKind::End,
                TokKind::Eof,
            ]
        );
        assert_eq!(
            kinds("'a'\n"),
            vec![TokKind::Char('a' as u32), TokKind::End, TokKind::Eof]
        );
        assert_eq!(
            kinds("'\\n'\n"),
            vec![TokKind::Char('\n' as u32), TokKind::End, TokKind::Eof]
        );
    }

    #[test]
    fn line_continuation_suppresses_end() {
        let ks = kinds("a\n  .b\n");
        assert_eq!(
            ks,
            vec![
                TokKind::Ident("a".into()),
                TokKind::Dot,
                TokKind::Ident("b".into()),
                TokKind::End,
                TokKind::Eof,
            ]
        );
    }
}
