//! 字句解析: 入力バイト列 → トークン列 (`docs/impl/02-frontend.md` §1)。
//!
//! 文の終端は **Go スタイル** (`draft.md` §4): 改行が暗黙の終端 ([`TokKind::End`])。
//! ブロックは `{}` でインデントは意味を持たない (非 Python)。行頭が `.`/二項演算子
//! なら前行の継続とみなし `End` を挿入しない (複数行メソッドチェーン)。
//!
//! M0 の範囲: `fn` / `return` / 識別子 / 整数 / `:=` `=` `->` 区切り / 四則演算。

use align_diag::Diagnostics;
use align_span::{FileId, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokKind {
    // リテラル・識別子
    Int(i128),
    Ident(String),
    // キーワード (M0 範囲)
    Fn,
    Return,
    Mut,
    Pub,
    Module,
    Import,
    // 記号・演算子
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
    /// 文の終端 (改行による暗黙の `;`、または明示 `;`)。
    End,
    Eof,
}

impl TokKind {
    /// このトークンが行末にあるとき、文を終え得るか (暗黙 End 挿入の判定)。
    fn can_end_stmt(&self) -> bool {
        matches!(
            self,
            TokKind::Int(_)
                | TokKind::Ident(_)
                | TokKind::Return
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

/// `source` をトークン列に変換する。末尾は必ず [`TokKind::Eof`]。
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
        // 最後の文に End を補い、Eof を置く。
        self.maybe_insert_end();
        let at = self.pos;
        self.tokens.push(Token {
            kind: TokKind::Eof,
            span: self.span(at, at),
        });
    }

    /// 改行に達したとき、直前トークンが文を終え得るなら暗黙 End を挿入する。
    /// ただし次の意味のあるバイトが行継続 (`.` または二項演算子) なら挿入しない。
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

    /// 次の意味のあるバイト (空白・コメント・改行を飛ばした先) が、前行の継続を
    /// 示す `.` または二項演算子で始まるか。
    fn next_significant_continues_line(&self) -> bool {
        let mut i = self.pos;
        loop {
            match self.src.get(i).copied() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => i += 1,
                Some(b'.') | Some(b'+') | Some(b'*') | Some(b'/') | Some(b'%') => return true,
                // '-' は単項にもなり得るが、行頭継続では二項とみなす。
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
                b'_' => self.pos += 1, // 桁区切り
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
            (b'=', _) => (TokKind::Eq, 1),
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
                    format!("予期しない文字: '{}'", c as char),
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
    fn line_continuation_suppresses_end() {
        // 行頭が '.' なら継続。End を挟まない。
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
