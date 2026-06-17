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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokKind {
    // Literals / identifiers
    Int(i128),
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
    // Symbols / operators
    ColonEq, // :=
    Eq,      // =
    Arrow,   // ->
    LParen,
    RParen,
    LBrace,
    RBrace,
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
                | TokKind::Ident(_)
                | TokKind::Return
                | TokKind::True
                | TokKind::False
                | TokKind::RParen
                | TokKind::RBrace
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
            b'0'..=b'9' => self.lex_number(start),
            c if is_ident_start(c) => self.lex_ident(start),
            _ => self.lex_symbol(start, diags),
        }
    }

    fn lex_number(&mut self, start: usize) {
        let mut value: i128 = 0;
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' => {
                    value = value * 10 + (c - b'0') as i128;
                    self.pos += 1;
                }
                b'_' => self.pos += 1, // digit separator
                _ => break,
            }
        }
        self.push(TokKind::Int(value), start);
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
            (b',', _) => (TokKind::Comma, 1),
            (b':', _) => (TokKind::Colon, 1),
            (b'.', _) => (TokKind::Dot, 1),
            (b'+', _) => (TokKind::Plus, 1),
            (b'-', _) => (TokKind::Minus, 1),
            (b'*', _) => (TokKind::Star, 1),
            (b'/', _) => (TokKind::Slash, 1),
            (b'%', _) => (TokKind::Percent, 1),
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
