//! Parsing: token stream -> AST (`docs/impl/02-frontend.md` §10).
//!
//! Hand-written recursive descent + Pratt parsing for expressions. No desugaring (the
//! AST keeps the written form). M1 scope: `fn` decls with multiple params, calls,
//! `if`/`else` (expression and statement), comparison/logical operators, `bool`,
//! `mut` + reassignment, integer arithmetic, and `( )` grouping.

use align_ast::*;
use align_diag::Diagnostics;
use align_lexer::{TokKind, Token};
use align_span::Span;

/// Whether `s` is a valid identifier (so `{ and {` or `{}` stay literal text).
fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Split a template literal's decoded content into static text and `{ident}` holes.
/// M5: only `{ident}` (no `{expr}`); the hole name is trimmed.
fn split_template(content: &str, span: Span) -> Vec<TemplatePart> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            let mut found_close = false;
            for d in chars.by_ref() {
                if d == '}' {
                    found_close = true;
                    break;
                }
                name.push(d);
            }
            let trimmed = name.trim();
            if found_close && is_ident(trimmed) {
                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                parts.push(TemplatePart::Hole(Ident { name: trimmed.to_string(), span }));
            } else {
                // Unmatched `{` or an empty `{}`: keep it as literal text.
                text.push('{');
                text.push_str(&name);
                if found_close {
                    text.push('}');
                }
            }
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        parts.push(TemplatePart::Text(text));
    }
    parts
}

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
                .error(format!("expected {what}"), self.span());
            false
        }
    }

    /// Skip statement separators (`End` from newlines or `;`).
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
        } else if matches!(self.peek(), TokKind::Ident(_)) && matches!(self.peek_at(1), TokKind::LBrace) {
            self.parse_struct(vis).map(Item::Struct)
        } else {
            self.diags
                .error("expected `fn` or a type declaration at top level", self.span());
            None
        }
    }

    /// `Name { field: Type, ... }` (keyword-less struct declaration). Fields are
    /// separated by commas and/or newlines. Sum types are deferred past M1.
    fn parse_struct(&mut self, vis: Vis) -> Option<StructDecl> {
        let start = self.span();
        let name = self.parse_ident("type name")?;
        self.expect(&TokKind::LBrace, "'{'");
        let mut fields = Vec::new();
        loop {
            self.skip_ends();
            if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                break;
            }
            let fstart = self.span();
            let fname = self.parse_ident("field name")?;
            if !self.eat(&TokKind::Colon) {
                self.diags
                    .error("expected ':' (only struct type declarations are supported)", self.span());
                return None;
            }
            let ty = self.parse_type()?;
            fields.push(FieldDef {
                name: fname,
                ty,
                span: fstart.merge(self.prev_span()),
            });
            // Separator: a comma and/or a newline (`End`); both are accepted.
            self.eat(&TokKind::Comma);
        }
        self.expect(&TokKind::RBrace, "'}'");
        let span = start.merge(self.prev_span());
        Some(StructDecl { vis, name, fields, span })
    }

    fn parse_fn(&mut self, vis: Vis) -> Option<FnDecl> {
        let start = self.span();
        self.bump(); // fn
        let name = self.parse_ident("function name")?;

        self.expect(&TokKind::LParen, "'('");
        let mut params = Vec::new();
        while !self.at(&TokKind::RParen) && !self.at(&TokKind::Eof) {
            let is_out = self.eat_ident_keyword("out");
            let pname = self.parse_ident("parameter name")?;
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
            // A `let`: `mut ...`, `name := ...`, or a type-annotated `name: T := ...`.
            // `name :` unambiguously starts a typed binding (no other statement does).
            if self.at(&TokKind::Mut)
                || (matches!(self.peek(), TokKind::Ident(_))
                    && matches!(self.peek_at(1), TokKind::ColonEq | TokKind::Colon))
            {
                let s = self.parse_let()?;
                stmts.push(s);
                continue;
            }
            if self.at(&TokKind::Return) {
                let s = self.parse_return()?;
                stmts.push(s);
                continue;
            }
            let e = self.parse_expr(0)?;
            if self.eat(&TokKind::Eq) {
                let value = self.parse_expr(0)?;
                self.eat(&TokKind::End);
                stmts.push(Stmt::Assign { place: e, value });
            } else if self.at(&TokKind::End) {
                self.bump();
                // A trailing expression (last thing before `}`) is the block's value,
                // even on its own line (newline inserts `End`).
                self.skip_ends();
                if self.at(&TokKind::RBrace) {
                    tail = Some(Box::new(e));
                    break;
                }
                stmts.push(Stmt::Expr(e));
            } else {
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
        let name = self.parse_ident("variable name")?;
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

    // --- expressions (Pratt) ---

    /// Binary operator binding power (higher binds tighter). `None` = not a binary op.
    fn binop(kind: &TokKind) -> Option<(BinOp, u8)> {
        Some(match kind {
            TokKind::OrOr => (BinOp::Or, 1),
            TokKind::AndAnd => (BinOp::And, 2),
            TokKind::EqEq => (BinOp::Eq, 3),
            TokKind::NotEq => (BinOp::Ne, 3),
            TokKind::Lt => (BinOp::Lt, 3),
            TokKind::Le => (BinOp::Le, 3),
            TokKind::Gt => (BinOp::Gt, 3),
            TokKind::Ge => (BinOp::Ge, 3),
            TokKind::Plus => (BinOp::Add, 4),
            TokKind::Minus => (BinOp::Sub, 4),
            TokKind::Star => (BinOp::Mul, 5),
            TokKind::Slash => (BinOp::Div, 5),
            TokKind::Percent => (BinOp::Rem, 5),
            _ => return None,
        })
    }

    fn parse_expr(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_prefix()?;
        loop {
            let Some((op, bp)) = Self::binop(self.peek()) else {
                break;
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
        // `opt else fallback` — Option unwrap, lowest precedence (top-level only, so it
        // never competes with `if … else`, whose else is consumed by `parse_if`).
        if min_bp == 0 && self.at(&TokKind::Else) {
            self.bump();
            let fallback = if self.at(&TokKind::LBrace) {
                let b = self.parse_block()?;
                let span = b.span;
                Expr { kind: ExprKind::Block(b), span }
            } else {
                self.parse_expr(0)?
            };
            let span = lhs.span.merge(fallback.span);
            lhs = Expr {
                kind: ExprKind::ElseUnwrap { opt: Box::new(lhs), fallback: Box::new(fallback) },
                span,
            };
        }
        Some(lhs)
    }

    fn parse_prefix(&mut self) -> Option<Expr> {
        let start = self.span();
        let op = match self.peek() {
            TokKind::Minus => Some(UnOp::Neg),
            TokKind::Bang => Some(UnOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let expr = self.parse_prefix()?;
            let span = start.merge(expr.span);
            return Some(Expr {
                kind: ExprKind::Unary {
                    op,
                    expr: Box::new(expr),
                },
                span,
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            if self.at(&TokKind::LParen) {
                self.bump();
                let mut args = Vec::new();
                while !self.at(&TokKind::RParen) && !self.at(&TokKind::Eof) {
                    args.push(self.parse_expr(0)?);
                    if !self.eat(&TokKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokKind::RParen, "')'");
                let span = e.span.merge(self.prev_span());
                e = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(e),
                        args,
                    },
                    span,
                };
            } else if self.at(&TokKind::Question) {
                self.bump();
                let span = e.span.merge(self.prev_span());
                e = Expr { kind: ExprKind::Try(Box::new(e)), span };
            } else if self.at(&TokKind::Dot) && matches!(self.peek_at(1), TokKind::Ident(_)) {
                self.bump(); // '.'
                let field = self.parse_ident("field or method name")?;
                let span = e.span.merge(field.span);
                e = Expr { kind: ExprKind::FieldAccess { recv: Box::new(e), field }, span };
            } else {
                break;
            }
        }
        Some(e)
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
            TokKind::Float(v) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Float(v),
                    span,
                })
            }
            TokKind::Char(v) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Char(v),
                    span,
                })
            }
            TokKind::Str(s) => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Str(s),
                    span,
                })
            }
            TokKind::True => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Bool(true),
                    span,
                })
            }
            TokKind::False => {
                self.bump();
                Some(Expr {
                    kind: ExprKind::Bool(false),
                    span,
                })
            }
            TokKind::Ident(_) => {
                // `Name { field: ... }` is a struct literal. Distinguish from a block
                // following a bare name (e.g. an `if` condition) by the `{ ident :`
                // shape — no valid statement-block starts that way.
                if matches!(self.peek_at(1), TokKind::LBrace)
                    && matches!(self.peek_at(2), TokKind::Ident(_))
                    && matches!(self.peek_at(3), TokKind::Colon)
                {
                    return self.parse_struct_lit();
                }
                // A single name; dotted access (`a.b`, method chains) is handled as a
                // postfix in `parse_postfix`.
                let id = self.parse_ident("identifier")?;
                let span = id.span;
                Some(Expr {
                    kind: ExprKind::Path(Path { segments: vec![id], span }),
                    span,
                })
            }
            TokKind::Dot if matches!(self.peek_at(1), TokKind::Ident(_)) => {
                // `.field` element-field shorthand (pipeline stage argument).
                self.bump();
                let field = self.parse_ident("field name")?;
                let span = span.merge(self.prev_span());
                Some(Expr { kind: ExprKind::FieldShorthand(field), span })
            }
            TokKind::Template => {
                self.bump();
                let str_span = self.span();
                let TokKind::Str(content) = self.peek().clone() else {
                    self.diags.error("expected a string literal after `template`", str_span);
                    return None;
                };
                self.bump();
                let parts = split_template(&content, str_span);
                let span = span.merge(self.prev_span());
                Some(Expr { kind: ExprKind::Template(parts), span })
            }
            TokKind::If => self.parse_if(),
            TokKind::Arena => {
                let start = self.span();
                self.bump();
                let block = self.parse_block()?;
                let span = start.merge(self.prev_span());
                Some(Expr { kind: ExprKind::Arena(block), span })
            }
            TokKind::LParen => {
                self.bump();
                // `()` is the unit value; otherwise a parenthesized expression.
                if self.at(&TokKind::RParen) {
                    self.bump();
                    let span = span.merge(self.prev_span());
                    return Some(Expr { kind: ExprKind::Unit, span });
                }
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
            TokKind::LBracket => {
                let start = self.span();
                self.bump();
                let mut elems = Vec::new();
                loop {
                    self.skip_ends();
                    if self.at(&TokKind::RBracket) || self.at(&TokKind::Eof) {
                        break;
                    }
                    elems.push(self.parse_expr(0)?);
                    self.skip_ends();
                    if !self.eat(&TokKind::Comma) {
                        break;
                    }
                }
                self.skip_ends();
                self.expect(&TokKind::RBracket, "']'");
                let span = start.merge(self.prev_span());
                Some(Expr { kind: ExprKind::ArrayLit(elems), span })
            }
            _ => {
                self.diags.error("expected expression", span);
                None
            }
        }
    }

    /// `Name { field: value, ... }`. Assumes the `Ident {` lookahead already matched.
    fn parse_struct_lit(&mut self) -> Option<Expr> {
        let start = self.span();
        let name = self.parse_ident("type name")?;
        self.expect(&TokKind::LBrace, "'{'");
        let mut fields = Vec::new();
        loop {
            self.skip_ends();
            if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                break;
            }
            let fstart = self.span();
            let fname = self.parse_ident("field name")?;
            self.expect(&TokKind::Colon, "':'");
            let value = self.parse_expr(0)?;
            fields.push(FieldInit {
                name: fname,
                value,
                span: fstart.merge(self.prev_span()),
            });
            self.eat(&TokKind::Comma);
        }
        self.expect(&TokKind::RBrace, "'}'");
        let span = start.merge(self.prev_span());
        Some(Expr {
            kind: ExprKind::StructLit { name, fields },
            span,
        })
    }

    fn parse_if(&mut self) -> Option<Expr> {
        let start = self.span();
        self.bump(); // if
        let cond = self.parse_expr(0)?;
        let then = self.parse_block()?;
        let els = if self.eat(&TokKind::Else) {
            if self.at(&TokKind::If) {
                Some(Box::new(self.parse_if()?))
            } else {
                let block = self.parse_block()?;
                let span = block.span;
                Some(Box::new(Expr {
                    kind: ExprKind::Block(block),
                    span,
                }))
            }
        } else {
            None
        };
        let span = start.merge(self.prev_span());
        Some(Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then,
                els,
            },
            span,
        })
    }

    // --- helpers ---

    fn parse_path(&mut self) -> Path {
        let start = self.span();
        let mut segments = Vec::new();
        if let Some(id) = self.parse_ident("identifier") {
            segments.push(id);
        }
        while self.at(&TokKind::Dot) && matches!(self.peek_at(1), TokKind::Ident(_)) {
            self.bump();
            if let Some(id) = self.parse_ident("identifier") {
                segments.push(id);
            }
        }
        Path {
            segments,
            span: start.merge(self.prev_span()),
        }
    }

    fn parse_type(&mut self) -> Option<Type> {
        // The unit type `()`.
        if self.at(&TokKind::LParen) {
            let start = self.span();
            self.bump();
            self.expect(&TokKind::RParen, "')'");
            let span = start.merge(self.prev_span());
            let seg = Ident { name: "()".to_string(), span };
            return Some(Type { path: Path { segments: vec![seg], span }, args: Vec::new(), span });
        }
        let path = self.parse_path();
        if path.segments.is_empty() {
            return None;
        }
        // Optional generic arguments: `Name<T, U>`. (`>>` lexes as two `>` tokens.)
        let mut args = Vec::new();
        if self.eat(&TokKind::Lt) {
            while !self.at(&TokKind::Gt) && !self.at(&TokKind::Eof) {
                args.push(self.parse_type()?);
                if !self.eat(&TokKind::Comma) {
                    break;
                }
            }
            self.expect(&TokKind::Gt, "'>'");
        }
        let span = path.span.merge(self.prev_span());
        Some(Type { path, args, span })
    }

    fn parse_ident(&mut self, what: &str) -> Option<Ident> {
        let span = self.span();
        if let TokKind::Ident(name) = self.peek().clone() {
            self.bump();
            Some(Ident { name, span })
        } else {
            self.diags.error(format!("expected {what}"), span);
            None
        }
    }

    /// Consume a weak keyword (one that appears as an `Ident`), like `out`.
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
        let Item::Fn(fd) = &f.items[0] else { panic!() };
        assert_eq!(fd.name.name, "main");
    }

    #[test]
    fn arithmetic_precedence() {
        let (f, err) = parse("fn f() -> i64 { return 1 + 2 * 3 }\n");
        assert!(!err);
        let Item::Fn(fd) = &f.items[0] else { panic!() };
        let FnBody::Block(b) = &fd.body else {
            panic!()
        };
        let Stmt::Return(Some(e)) = &b.stmts[0] else {
            panic!()
        };
        let ExprKind::Binary { op, .. } = &e.kind else {
            panic!()
        };
        assert_eq!(*op, BinOp::Add);
    }

    #[test]
    fn fib_parses() {
        let src = "fn fib(n: i64) -> i64 {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\n";
        let (f, err) = parse(src);
        assert!(!err);
        let Item::Fn(fd) = &f.items[0] else { panic!() };
        assert_eq!(fd.params.len(), 1);
        let FnBody::Block(b) = &fd.body else {
            panic!()
        };
        // if-statement, then return-with-call.
        assert!(matches!(&b.stmts[0], Stmt::Expr(Expr { kind: ExprKind::If { .. }, .. })));
        assert!(matches!(&b.stmts[1], Stmt::Return(Some(_))));
    }

    #[test]
    fn struct_decl_and_literal_parse() {
        let src = "Point { x: i32, y: i32 }\nfn main() -> i32 {\n  p := Point { x: 1, y: 2 }\n  return p.x\n}\n";
        let (f, err) = parse(src);
        assert!(!err);
        let Item::Struct(sd) = &f.items[0] else { panic!("expected struct decl") };
        assert_eq!(sd.name.name, "Point");
        assert_eq!(sd.fields.len(), 2);
        let Item::Fn(fd) = &f.items[1] else { panic!() };
        let FnBody::Block(b) = &fd.body else { panic!() };
        let Stmt::Let { init, .. } = &b.stmts[0] else { panic!() };
        assert!(matches!(init.kind, ExprKind::StructLit { .. }), "init should be a struct literal");
    }

    #[test]
    fn template_splits_holes_and_keeps_bad_braces_literal() {
        use crate::TemplatePart;
        let span = Span::new(0, 0, 0);
        // Valid hole.
        let p = split_template("a {x} b", span);
        assert!(matches!(p.as_slice(), [TemplatePart::Text(_), TemplatePart::Hole(_), TemplatePart::Text(_)]));
        // Unmatched `{`, empty `{}`, and non-ident `{ x y }` all stay literal text.
        for s in ["unmatched {", "empty {}", "{ x y } two words"] {
            let parts = split_template(s, span);
            assert!(
                parts.iter().all(|p| matches!(p, TemplatePart::Text(_))),
                "expected all-literal for {s:?}, got {parts:?}"
            );
        }
    }

    #[test]
    fn method_chain_parses_as_nested_field_access_calls() {
        // `a.f(x).g()` → Call(FieldAccess(Call(FieldAccess(a, f), [x]), g), [])
        let (f, err) = parse("fn main() -> i32 {\n  return a.f(1).g()\n}\n");
        assert!(!err);
        let Item::Fn(fd) = &f.items[0] else { panic!() };
        let FnBody::Block(b) = &fd.body else { panic!() };
        let Stmt::Return(Some(e)) = &b.stmts[0] else { panic!() };
        let ExprKind::Call { callee, .. } = &e.kind else { panic!("outer call") };
        let ExprKind::FieldAccess { recv, field } = &callee.kind else { panic!("outer .g") };
        assert_eq!(field.name, "g");
        assert!(matches!(recv.kind, ExprKind::Call { .. }), "receiver is the inner call");
    }

    #[test]
    fn name_block_in_if_condition_is_not_a_struct_literal() {
        // `if p { ... }` must parse `p` as the condition, not `p { ... }` as a literal.
        let (_f, err) = parse("fn f(p: bool) -> i32 {\n  if p { return 1 }\n  return 0\n}\n");
        assert!(!err);
    }
}
