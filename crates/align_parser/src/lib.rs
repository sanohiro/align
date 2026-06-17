//! 構文解析: トークン列 → AST (`docs/impl/02-frontend.md` §10)。
//!
//! 手書き再帰下降 + 式は Pratt parsing。脱糖はしない (AST は書かれた形を保つ)。
//! M0 範囲: `fn` 宣言 / `:=` / `return` / 整数 / 四則演算 / `( )` グループ化。

use align_ast::*;
use align_diag::Diagnostics;
use align_span::Span;
use align_lexer::{TokKind, Token};

pub fn parse_file(tokens: Vec<Token>, diags: &mut Diagnostics) -> File {
    let mut p = Parser {
        tokens,
        pos: 0,
        diags,
    };
    p.parse_file()
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    diags: &'a mut Diagnostics,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &TokKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, n: usize) -> &TokKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.tokens[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.kind, TokKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn at(&self, k: &TokKind) -> bool {
        self.peek() == k
    }

    fn eat(&mut self, k: &TokKind) -> bool {
        if self.at(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, k: &TokKind, what: &str) -> bool {
        if self.eat(k) {
            true
        } else {
            self.diags
                .error(format!("{what} を期待しました"), self.span());
            false
        }
    }

    /// 文の区切り (改行/`;` 由来の End) を読み飛ばす。
    fn skip_ends(&mut self) {
        while self.at(&TokKind::End) {
            self.bump();
        }
    }

    fn parse_file(&mut self) -> File {
        self.skip_ends();
        let module = if self.at(&TokKind::Module) {
            self.bump();
            let p = self.parse_path();
            self.eat(&TokKind::End);
            Some(p)
        } else {
            None
        };

        let mut imports = Vec::new();
        loop {
            self.skip_ends();
            if self.at(&TokKind::Import) {
                self.bump();
                imports.push(self.parse_path());
                self.eat(&TokKind::End);
            } else {
                break;
            }
        }

        let mut items = Vec::new();
        loop {
            self.skip_ends();
            if self.at(&TokKind::Eof) {
                break;
            }
            match self.parse_item() {
                Some(item) => items.push(item),
                None => {
                    // エラー回復: 次の同期点 (fn / End) まで読み飛ばす。
                    self.bump();
                    while !matches!(self.peek(), TokKind::Fn | TokKind::Eof | TokKind::End) {
                        self.bump();
                    }
                }
            }
        }

        File {
            module,
            imports,
            items,
        }
    }

    fn parse_item(&mut self) -> Option<Item> {
        let vis = if self.eat(&TokKind::Pub) {
            Vis::Pub
        } else {
            Vis::Private
        };
        if self.at(&TokKind::Fn) {
            self.parse_fn(vis).map(Item::Fn)
        } else {
            self.diags
                .error("トップレベルでは fn を期待しました", self.span());
            None
        }
    }

    fn parse_fn(&mut self, vis: Vis) -> Option<FnDecl> {
        let start = self.span();
        self.bump(); // fn
        let name = self.parse_ident("関数名")?;

        self.expect(&TokKind::LParen, "'('");
        let mut params = Vec::new();
        while !self.at(&TokKind::RParen) && !self.at(&TokKind::Eof) {
            let is_out = self.eat_ident_keyword("out");
            let pname = self.parse_ident("引数名")?;
            self.expect(&TokKind::Colon, "':'");
            let ty = self.parse_type()?;
            params.push(Param {
                is_out,
                name: pname,
                ty,
            });
            if !self.eat(&TokKind::Comma) {
                break;
            }
        }
        self.expect(&TokKind::RParen, "')'");

        let ret = if self.eat(&TokKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = if self.eat(&TokKind::Eq) {
            // 単一式形 `= expr`
            let e = self.parse_expr(0)?;
            self.eat(&TokKind::End);
            FnBody::Expr(Box::new(e))
        } else {
            FnBody::Block(self.parse_block()?)
        };

        let span = start.merge(self.prev_span());
        Some(FnDecl {
            vis,
            name,
            params,
            ret,
            body,
            span,
        })
    }

    fn parse_block(&mut self) -> Option<Block> {
        let start = self.span();
        self.expect(&TokKind::LBrace, "'{'");
        let mut stmts = Vec::new();
        let mut tail: Option<Box<Expr>> = None;

        loop {
            self.skip_ends();
            if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                break;
            }
            // let
            if self.at(&TokKind::Mut)
                || (matches!(self.peek(), TokKind::Ident(_))
                    && matches!(self.peek_at(1), TokKind::ColonEq))
            {
                let s = self.parse_let()?;
                stmts.push(s);
                continue;
            }
            // return
            if self.at(&TokKind::Return) {
                let s = self.parse_return()?;
                stmts.push(s);
                continue;
            }
            // expr → 代入 / 式文 / 末尾式
            let e = self.parse_expr(0)?;
            if self.eat(&TokKind::Eq) {
                let value = self.parse_expr(0)?;
                self.eat(&TokKind::End);
                stmts.push(Stmt::Assign { place: e, value });
            } else if self.at(&TokKind::End) {
                self.bump();
                stmts.push(Stmt::Expr(e));
            } else {
                // End なしでブロック末 → 末尾式
                tail = Some(Box::new(e));
                break;
            }
        }

        self.expect(&TokKind::RBrace, "'}'");
        let span = start.merge(self.prev_span());
        Some(Block { stmts, tail, span })
    }

    fn parse_let(&mut self) -> Option<Stmt> {
        let is_mut = self.eat(&TokKind::Mut);
        let name = self.parse_ident("変数名")?;
        let ty = if self.eat(&TokKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokKind::ColonEq, "':='");
        let init = self.parse_expr(0)?;
        self.eat(&TokKind::End);
        Some(Stmt::Let {
            is_mut,
            name,
            ty,
            init,
        })
    }

    fn parse_return(&mut self) -> Option<Stmt> {
        self.bump(); // return
        let value = if self.at(&TokKind::End) || self.at(&TokKind::RBrace) {
            None
        } else {
            Some(self.parse_expr(0)?)
        };
        self.eat(&TokKind::End);
        Some(Stmt::Return(value))
    }

    // --- 式 (Pratt) ---

    fn parse_expr(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_primary()?;
        loop {
            let (op, bp) = match self.peek() {
                TokKind::Plus => (BinOp::Add, 1),
                TokKind::Minus => (BinOp::Sub, 1),
                TokKind::Star => (BinOp::Mul, 2),
                TokKind::Slash => (BinOp::Div, 2),
                TokKind::Percent => (BinOp::Rem, 2),
                _ => break,
            };
            if bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.parse_expr(bp + 1)?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Some(lhs)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let span = self.span();
        match self.peek().clone() {
            TokKind::Int(v) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Int(v),
                    span,
                })
            }
            TokKind::Ident(_) => {
                let path = self.parse_path();
                let span = path.span;
                Some(Expr {
                    kind: ExprKind::Path(path),
                    span,
                })
            }
            TokKind::LParen => {
                self.bump();
                let e = self.parse_expr(0)?;
                self.expect(&TokKind::RParen, "')'");
                Some(e)
            }
            TokKind::LBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Some(Expr {
                    kind: ExprKind::Block(block),
                    span,
                })
            }
            _ => {
                self.diags.error("式を期待しました", span);
                None
            }
        }
    }

    // --- 補助 ---

    fn parse_path(&mut self) -> Path {
        let start = self.span();
        let mut segments = Vec::new();
        if let Some(id) = self.parse_ident("識別子") {
            segments.push(id);
        }
        while self.at(&TokKind::Dot) && matches!(self.peek_at(1), TokKind::Ident(_)) {
            self.bump(); // .
            if let Some(id) = self.parse_ident("識別子") {
                segments.push(id);
            }
        }
        Path {
            segments,
            span: start.merge(self.prev_span()),
        }
    }

    fn parse_type(&mut self) -> Option<Type> {
        let path = self.parse_path();
        if path.segments.is_empty() {
            return None;
        }
        let span = path.span;
        Some(Type { path, span })
    }

    fn parse_ident(&mut self, what: &str) -> Option<Ident> {
        let span = self.span();
        if let TokKind::Ident(name) = self.peek().clone() {
            self.bump();
            Some(Ident { name, span })
        } else {
            self.diags.error(format!("{what} を期待しました"), span);
            None
        }
    }

    /// `out` のような弱いキーワード (Ident として現れる) を消費する。
    fn eat_ident_keyword(&mut self, kw: &str) -> bool {
        if let TokKind::Ident(name) = self.peek() {
            if name == kw {
                self.bump();
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_lexer::tokenize;

    fn parse(src: &str) -> (File, bool) {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        (f, d.has_errors())
    }

    #[test]
    fn m0_program_parses() {
        let (f, err) = parse("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        assert!(!err);
        assert_eq!(f.items.len(), 1);
        let Item::Fn(fd) = &f.items[0];
        assert_eq!(fd.name.name, "main");
        assert_eq!(fd.ret.as_ref().unwrap().path.segments[0].name, "i32");
        let FnBody::Block(b) = &fd.body else {
            panic!("block body expected")
        };
        assert_eq!(b.stmts.len(), 2);
    }

    #[test]
    fn arithmetic_precedence() {
        let (f, err) = parse("fn f() -> i64 { return 1 + 2 * 3 }\n");
        assert!(!err);
        let Item::Fn(fd) = &f.items[0];
        let FnBody::Block(b) = &fd.body else {
            panic!()
        };
        let Stmt::Return(Some(e)) = &b.stmts[0] else {
            panic!()
        };
        // 最上位は + で、右が * (1 + (2*3))
        let ExprKind::Binary { op, .. } = &e.kind else {
            panic!()
        };
        assert_eq!(*op, BinOp::Add);
    }
}
