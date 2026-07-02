//! Parsing: token stream -> AST (`docs/impl/02-frontend.md` §10).
//!
//! Hand-written recursive descent + Pratt parsing for expressions. No desugaring (the
//! AST keeps the written form). M1 scope: `fn` decls with multiple params, calls,
//! `if`/`else` (expression and statement), comparison/logical operators, `bool`,
//! `mut` + reassignment, integer arithmetic, and `( )` grouping.

use align_ast::*;
use align_diag::{Diagnostic, Diagnostics};
use align_lexer::{TokKind, Token};
use align_span::Span;

/// Whether `s` is a valid identifier (so `{ and {` or `{}` stay literal text).
/// A raw template piece, before holes are parsed: either literal text or the (untrimmed)
/// source of a `{...}` hole plus its byte offset within the decoded content (for span
/// remapping). Splitting is kept pure (no parsing) so it can be unit-tested on its own.
#[derive(Debug)]
enum RawPart {
    Text(String),
    Hole { src: String, off: usize },
}

/// Split a template literal's decoded content into static text and `{...}` holes. A hole is
/// any non-empty `{...}` (its contents are parsed as an expression later). An unmatched `{`
/// or an empty `{}` stays literal text. The hole body runs to the first `}` (nested braces,
/// e.g. struct literals, are out of scope for M5).
fn split_template(content: &str) -> Vec<RawPart> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = content.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c == '{' {
            let off = idx + 1; // byte offset of the hole's first char within `content`
            let mut src = String::new();
            let mut found_close = false;
            for (_, d) in chars.by_ref() {
                if d == '}' {
                    found_close = true;
                    break;
                }
                src.push(d);
            }
            if found_close && !src.trim().is_empty() {
                if !text.is_empty() {
                    parts.push(RawPart::Text(std::mem::take(&mut text)));
                }
                parts.push(RawPart::Hole { src, off });
            } else {
                // Unmatched `{` or an empty `{}`: keep it as literal text.
                text.push('{');
                text.push_str(&src);
                if found_close {
                    text.push('}');
                }
            }
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        parts.push(RawPart::Text(text));
    }
    parts
}

pub fn parse_file(tokens: Vec<Token>, diags: &mut Diagnostics) -> File {
    let mut file = {
        let mut p = Parser {
            tokens,
            pos: 0,
            diags,
            no_struct_literal: false,
            depth: 0,
        };
        p.parse_file()
    };
    // Guard 2 (see MAX_EXPR_DEPTH): the recursion guards above only bound *recursively* parsed
    // nesting; iteratively-built chains (`1+1+1+…`, `a.f().g()…`) can still be arbitrarily deep.
    // Re-check the finished tree and truncate any over-deep expression so downstream recursive
    // passes (sema/MIR/codegen) can't overflow the stack.
    cap_expr_depths(&mut file, diags);
    file
}

/// Walk the finished AST and truncate any expression nested deeper than [`MAX_EXPR_DEPTH`],
/// replacing the offending sub-tree with a `Unit` placeholder and emitting the same
/// "expression nests too deeply" diagnostic the parser's recursion guards use. This is the second
/// half of the depth ceiling: it catches trees the left-associative binary loop and the postfix
/// loop grow *iteratively* (which consume no parser recursion budget). The walk itself recurses at
/// most `MAX_EXPR_DEPTH` levels — it stops descending at a truncation point — so it is stack-safe
/// on any input.
fn cap_expr_depths(file: &mut File, diags: &mut Diagnostics) {
    for item in &mut file.items {
        match item {
            Item::Fn(f) => match &mut f.body {
                FnBody::Block(b) => cap_block_depth(b, 1, diags),
                FnBody::Expr(e) => cap_expr_depth(e, 1, diags),
            },
            Item::Const(c) => cap_expr_depth(&mut c.value, 1, diags),
            // Structs / enums / extern blocks carry no expression bodies.
            Item::Struct(_) | Item::Enum(_) | Item::Extern(_) => {}
        }
    }
}

/// An expression node with no sub-expression children — descending never recurses through it.
fn is_leaf_expr(k: &ExprKind) -> bool {
    matches!(
        k,
        ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Path(_)
            | ExprKind::FieldShorthand(_)
    )
}

/// Truncate `e` (and its descendants) to at most `MAX_EXPR_DEPTH` levels, `depth` being `e`'s own
/// nesting level (root = 1). Returns after truncating without descending, so recursion is bounded.
fn cap_expr_depth(e: &mut Expr, depth: u32, diags: &mut Diagnostics) {
    if depth > MAX_EXPR_DEPTH {
        // A leaf that happens to land one past the ceiling is harmless — downstream visits it in a
        // single frame with no recursion below it — so it needs neither truncation nor a
        // diagnostic. Report + cut only the compound that actually carries the over-deep sub-tree,
        // which yields exactly one clean diagnostic per over-deep chain (not one per sibling leaf).
        if !is_leaf_expr(&e.kind) {
            diags.error("expression nests too deeply", e.span);
            e.kind = ExprKind::Unit;
        }
        return;
    }
    let d = depth + 1;
    match &mut e.kind {
        ExprKind::Unit
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Path(_)
        | ExprKind::FieldShorthand(_) => {}
        ExprKind::Unary { expr, .. }
        | ExprKind::Cast { expr, .. }
        | ExprKind::Try(expr)
        | ExprKind::FieldAccess { recv: expr, .. }
        | ExprKind::TupleIndex { recv: expr, .. } => cap_expr_depth(expr, d, diags),
        ExprKind::Binary { lhs, rhs, .. } => {
            cap_expr_depth(lhs, d, diags);
            cap_expr_depth(rhs, d, diags);
        }
        ExprKind::Call { callee, args } => {
            cap_expr_depth(callee, d, diags);
            for a in args {
                cap_expr_depth(a, d, diags);
            }
        }
        ExprKind::If { cond, then, els } => {
            cap_expr_depth(cond, d, diags);
            cap_block_depth(then, d, diags);
            if let Some(e) = els {
                cap_expr_depth(e, d, diags);
            }
        }
        ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::Unsafe(b) | ExprKind::TaskGroup(b) => {
            cap_block_depth(b, d, diags)
        }
        ExprKind::Lambda { body, .. } => cap_block_depth(body, d, diags),
        ExprKind::StructLit { fields, .. } => {
            for f in fields {
                cap_expr_depth(&mut f.value, d, diags);
            }
        }
        ExprKind::ElseUnwrap { opt, fallback } => {
            cap_expr_depth(opt, d, diags);
            cap_expr_depth(fallback, d, diags);
        }
        ExprKind::ArrayLit(es) | ExprKind::Tuple(es) => {
            for x in es {
                cap_expr_depth(x, d, diags);
            }
        }
        ExprKind::Index { recv, index } => {
            cap_expr_depth(recv, d, diags);
            cap_expr_depth(index, d, diags);
        }
        ExprKind::SliceRange { recv, start, end } => {
            cap_expr_depth(recv, d, diags);
            if let Some(s) = start {
                cap_expr_depth(s, d, diags);
            }
            if let Some(en) = end {
                cap_expr_depth(en, d, diags);
            }
        }
        ExprKind::Match { scrutinee, arms } => {
            cap_expr_depth(scrutinee, d, diags);
            for a in arms {
                cap_expr_depth(&mut a.body, d, diags);
            }
        }
        ExprKind::Template(parts) => {
            for p in parts {
                if let TemplatePart::Hole(h) = p {
                    cap_expr_depth(h, d, diags);
                }
            }
        }
    }
}

/// Truncate the expressions inside a block to at most `MAX_EXPR_DEPTH` levels. A block deepens the
/// nesting by one (downstream recurses block → statement → expression), so its statements/tail are
/// checked at `depth + 1`. When the block itself is already at the ceiling, drop its contents.
fn cap_block_depth(b: &mut Block, depth: u32, diags: &mut Diagnostics) {
    if depth > MAX_EXPR_DEPTH {
        diags.error("expression nests too deeply", b.span);
        b.stmts.clear();
        b.tail = None;
        return;
    }
    let d = depth + 1;
    for s in &mut b.stmts {
        match s {
            Stmt::Let { init, .. } => cap_expr_depth(init, d, diags),
            Stmt::LetTuple { init, .. } => cap_expr_depth(init, d, diags),
            Stmt::Assign { place, value } => {
                cap_expr_depth(place, d, diags);
                cap_expr_depth(value, d, diags);
            }
            Stmt::Return(Some(e)) => cap_expr_depth(e, d, diags),
            Stmt::Return(None) => {}
            Stmt::Expr(e) => cap_expr_depth(e, d, diags),
        }
    }
    if let Some(t) = &mut b.tail {
        cap_expr_depth(t, d, diags);
    }
}

/// Maximum AST nesting depth of any one expression handed downstream. It bounds BOTH:
///   1. parser recursion (`parse_expr`/`parse_prefix`) — deeply *recursive* input (`((((…))))`, a
///      unary chain `----…`) would overflow the parser's own native stack mid-parse; the guards
///      below stop at this ceiling and emit one diagnostic; and
///   2. the depth of the *built* tree — the left-associative binary loop and the postfix-method loop
///      grow the AST **iteratively** (no parser recursion, so they slip past guard 1), yet every
///      downstream pass walks that tree recursively. [`cap_expr_depths`] re-checks the finished AST
///      against this same ceiling and truncates anything deeper, so no over-deep tree ever reaches
///      sema / MIR / codegen.
///
/// Chosen from measured stack limits (debug, unoptimized): the heaviest downstream pass, MIR
/// lowering, overflows at depth ~275 on the 8 MB main thread (where full builds run) and sema
/// overflows at ~235 on a 2 MB worker/test thread (where the fuzz/test `parse→sema` path runs). 128
/// leaves ~2x headroom on both while still admitting reasonably deep machine-generated code (a
/// 128-term expression).
const MAX_EXPR_DEPTH: u32 = 128;

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    diags: &'a mut Diagnostics,
    /// While parsing an `if`/`match` condition, a bare `Name { … }` is the header's block, not a
    /// struct literal — so struct-literal recognition is suppressed at the condition's top level
    /// (a struct literal there must be parenthesized, `if (P { x: 1 }).ok { … }`). Cleared inside
    /// any delimiter (`(...)`, `[...]`, call args) where the block ambiguity can't arise.
    no_struct_literal: bool,
    /// Current expression-recursion depth (see [`MAX_EXPR_DEPTH`]).
    depth: u32,
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

    /// Require a statement terminator after a `let` / `return` / assignment: an `End` (a newline or
    /// `;`) or the end of the enclosing block (`}` / EOF). Enforces the Go-style "a newline
    /// terminates a statement" rule so two statements can't silently run together on one line
    /// without a `;` (`x := 1 return x`). Consumes the `End` (and any run of them) when present.
    fn expect_stmt_end(&mut self) {
        if self.at(&TokKind::End) {
            self.skip_ends();
        } else if !self.at(&TokKind::RBrace) && !self.at(&TokKind::Eof) {
            self.diags.error("expected a newline or `;` to separate statements".to_string(), self.span());
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
        if self.at(&TokKind::Extern) {
            // `extern "C" fn ...` — a foreign declaration is inherently a global C symbol, so `pub`
            // is meaningless here (an extern is never a module export).
            if matches!(vis, Vis::Pub) {
                self.diags
                    .error("`pub` is not allowed on an `extern` declaration (a foreign symbol is already global)", self.span());
            }
            self.parse_extern().map(Item::Extern)
        } else if self.at(&TokKind::Fn) {
            self.parse_fn(vis).map(Item::Fn)
        } else if self.at(&TokKind::Mut) {
            // A top-level constant is immutable; `mut` is only for local bindings.
            self.diags
                .error("a top-level constant is immutable; `mut` is not allowed here", self.span());
            None
        } else if matches!(self.peek(), TokKind::Ident(s) if s == "align" || s == "layout")
            && matches!(self.peek_at(1), TokKind::LParen)
        {
            // A leading run of type attributes: `align(N)` (over-alignment) and/or `layout(C)`
            // (C-compatible flat layout), in any order, before a type declaration.
            let (align, c_repr) = self.parse_type_attrs();
            self.parse_type_decl(vis, align, c_repr)
        } else if matches!(self.peek(), TokKind::Ident(_))
            && matches!(self.peek_at(1), TokKind::LBrace | TokKind::Lt)
        {
            // `Name { … }` or a generic `Name<T> { … }` type declaration.
            self.parse_type_decl(vis, None, false)
        } else if matches!(self.peek(), TokKind::Ident(_))
            && matches!(self.peek_at(1), TokKind::ColonEq | TokKind::Colon)
        {
            // `NAME := expr` / `NAME: Type := expr` — a top-level named constant.
            self.parse_const(vis)
        } else {
            self.diags
                .error("expected `fn`, a type declaration, or a constant (`NAME := …`) at top level", self.span());
            None
        }
    }

    /// A top-level constant `NAME := expr` / `NAME: Type := expr`. Mirrors a local `let` minus `mut`
    /// (rejected at the call site). The value is evaluated at compile time in sema.
    fn parse_const(&mut self, vis: Vis) -> Option<Item> {
        let start = self.span();
        let name = self.parse_ident("constant name")?;
        let ty = if self.eat(&TokKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokKind::ColonEq, "':='");
        let value = self.parse_expr(0)?;
        self.eat(&TokKind::End);
        let span = start.merge(self.prev_span());
        Some(Item::Const(ConstDecl { vis, name, ty, value, span }))
    }

    /// A keyword-less type declaration `Name { … }`, disambiguated by content: a body of
    /// `field: Type` is a struct; a body of bare `Variant` names is a sum type. (S1a: tag-only
    /// variants.)
    /// Parse an `align(N)` attribute prefix → the byte alignment `N` (a positive power of two).
    /// Returns `None` (with a diagnostic) on a malformed / non-power-of-two / too-large value, so the
    /// type declaration still parses — just without the over-alignment.
    /// A leading run of type-declaration attributes — `align(N)` and/or `layout(C)`, in any order.
    /// Returns the collected over-alignment and whether `layout(C)` was present.
    fn parse_type_attrs(&mut self) -> (Option<u32>, bool) {
        let mut align = None;
        let mut c_repr = false;
        loop {
            match self.peek() {
                TokKind::Ident(s) if s == "align" && matches!(self.peek_at(1), TokKind::LParen) => {
                    if let Some(a) = self.parse_align_attr() {
                        align = Some(a);
                    }
                }
                TokKind::Ident(s) if s == "layout" && matches!(self.peek_at(1), TokKind::LParen) => {
                    self.parse_layout_attr(&mut c_repr);
                }
                _ => break,
            }
        }
        (align, c_repr)
    }

    /// `layout(C)` — the only supported layout attribute. Sets `c_repr`.
    fn parse_layout_attr(&mut self, c_repr: &mut bool) {
        self.bump(); // `layout`
        self.expect(&TokKind::LParen, "'(' after `layout`");
        let span = self.span();
        match self.peek() {
            TokKind::Ident(s) if s == "C" => {
                self.bump();
                *c_repr = true;
            }
            _ => {
                self.diags.error("`layout(C)` is the only supported layout attribute", span);
                // Consume the offending kind (if any) so the `)` below still matches — one clean
                // error, no cascade.
                if !self.at(&TokKind::RParen) && !self.at(&TokKind::Eof) {
                    self.bump();
                }
            }
        }
        self.expect(&TokKind::RParen, "')'");
    }

    fn parse_align_attr(&mut self) -> Option<u32> {
        self.bump(); // `align`
        self.expect(&TokKind::LParen, "'(' after `align`");
        let span = self.span();
        let n = if let TokKind::Int(v) = self.peek() {
            let v = *v;
            self.bump();
            v
        } else {
            self.diags.error("`align(N)` needs an integer alignment", span);
            self.eat(&TokKind::RParen);
            return None;
        };
        self.expect(&TokKind::RParen, "')'");
        if n <= 0 || !(n as u128).is_power_of_two() {
            self.diags.error(format!("an alignment must be a positive power of two, got {n}"), span);
            return None;
        }
        match u32::try_from(n) {
            Ok(v) if v <= (1 << 29) => Some(v),
            _ => {
                self.diags.error(format!("alignment {n} is too large"), span);
                None
            }
        }
    }

    fn parse_type_decl(&mut self, vis: Vis, align: Option<u32>, c_repr: bool) -> Option<Item> {
        let start = self.span();
        let name = self.parse_ident("type name")?;
        // Optional generic type parameters: `Pair<T, U: Ord>` (same form as a function's).
        let mut type_params = Vec::new();
        if self.eat(&TokKind::Lt) {
            while !self.at(&TokKind::Gt) && !self.at(&TokKind::Eof) {
                let tname = self.parse_ident("a type parameter name")?;
                let bound = if self.eat(&TokKind::Colon) {
                    Some(self.parse_ident("a bound (Eq, Ord, or Num)")?)
                } else {
                    None
                };
                type_params.push(TypeParam { name: tname, bound });
                if !self.eat(&TokKind::Comma) {
                    break;
                }
            }
            self.expect(&TokKind::Gt, "'>'");
        }
        self.expect(&TokKind::LBrace, "'{'");
        self.skip_ends();
        // `ident :` → struct; an empty body or `ident ,`/`ident }` → struct (no fields) / sum type.
        let is_struct = self.at(&TokKind::RBrace) || matches!(self.peek_at(1), TokKind::Colon);
        if is_struct {
            let mut fields = Vec::new();
            loop {
                self.skip_ends();
                if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                    break;
                }
                let fstart = self.span();
                let fname = self.parse_ident("field name")?;
                self.expect(&TokKind::Colon, "':'");
                let ty = self.parse_type()?;
                fields.push(FieldDef { name: fname, ty, span: fstart.merge(self.prev_span()) });
                self.eat(&TokKind::Comma);
            }
            self.expect(&TokKind::RBrace, "'}'");
            let span = start.merge(self.prev_span());
            Some(Item::Struct(StructDecl { vis, name, type_params, fields, align, c_repr, span }))
        } else {
            if align.is_some() {
                self.diags.error("`align(N)` applies to a struct, not a sum type", start);
            }
            if c_repr {
                self.diags.error("`layout(C)` applies to a struct, not a sum type", start);
            }
            let mut variants = Vec::new();
            loop {
                self.skip_ends();
                if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                    break;
                }
                let vstart = self.span();
                let vname = self.parse_ident("variant name")?;
                // An optional positional payload: `Variant(T, U, …)`.
                let mut payload = Vec::new();
                if self.eat(&TokKind::LParen) {
                    loop {
                        self.skip_ends();
                        if self.at(&TokKind::RParen) || self.at(&TokKind::Eof) {
                            break;
                        }
                        payload.push(self.parse_type()?);
                        if !self.eat(&TokKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokKind::RParen, "')'");
                }
                variants.push(VariantDef { name: vname, payload, span: vstart.merge(self.prev_span()) });
                self.eat(&TokKind::Comma);
            }
            self.expect(&TokKind::RBrace, "'}'");
            let span = start.merge(self.prev_span());
            Some(Item::Enum(EnumDecl { vis, name, type_params, variants, span }))
        }
    }

    fn parse_fn(&mut self, vis: Vis) -> Option<FnDecl> {
        let start = self.span();
        self.bump(); // fn
        let name = self.parse_ident("function name")?;

        // Optional generic type parameters: `fn f<T, U: Ord>(...)`.
        let mut type_params = Vec::new();
        if self.eat(&TokKind::Lt) {
            while !self.at(&TokKind::Gt) && !self.at(&TokKind::Eof) {
                let name = self.parse_ident("a type parameter name")?;
                // Optional builtin bound: `T: Ord`.
                let bound = if self.eat(&TokKind::Colon) {
                    Some(self.parse_ident("a bound (Eq, Ord, or Num)")?)
                } else {
                    None
                };
                type_params.push(TypeParam { name, bound });
                if !self.eat(&TokKind::Comma) {
                    break;
                }
            }
            self.expect(&TokKind::Gt, "'>'");
        }

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
            type_params,
            params,
            ret,
            body,
            span,
        })
    }

    /// `extern "C" fn f(...) -> T` or a braced group `extern "C" { fn ...\n fn ... }`. Both forms
    /// produce one [`ExternBlock`] carrying one-or-more bodyless [`ExternSig`]s.
    fn parse_extern(&mut self) -> Option<ExternBlock> {
        let start = self.span();
        self.bump(); // extern
        // The ABI string (`"C"`). Kept as a string here; sema validates it is `"C"`.
        let abi = match self.peek() {
            TokKind::Str(_) => {
                let TokKind::Str(s) = self.bump().kind else { unreachable!() };
                s
            }
            _ => {
                self.diags.error("expected an ABI string after `extern` (e.g. `extern \"C\"`)", self.span());
                return None;
            }
        };
        // Optional `link("name")` clause naming the external library to link (`-lname`). `link` is a
        // contextual identifier here (like `align`/`layout`), so no keyword is reserved.
        let link = if matches!(self.peek(), TokKind::Ident(s) if s == "link")
            && matches!(self.peek_at(1), TokKind::LParen)
        {
            self.parse_link_clause()
        } else {
            None
        };
        let mut fns = Vec::new();
        if self.eat(&TokKind::LBrace) {
            // Braced group: any number of `fn` signatures, one per line.
            loop {
                self.skip_ends();
                if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                    break;
                }
                fns.push(self.parse_extern_sig()?);
            }
            self.expect(&TokKind::RBrace, "'}'");
        } else {
            // Single-signature form: `extern "C" fn f(...) -> T`.
            fns.push(self.parse_extern_sig()?);
        }
        let span = start.merge(self.prev_span());
        Some(ExternBlock { abi, link, fns, span })
    }

    /// `link("name")` — the external library for an `extern` block. The name is validated in sema.
    fn parse_link_clause(&mut self) -> Option<String> {
        self.bump(); // `link`
        self.expect(&TokKind::LParen, "'(' after `link`");
        let name = match self.peek() {
            TokKind::Str(_) => {
                let TokKind::Str(s) = self.bump().kind else { unreachable!() };
                Some(s)
            }
            // Empty `link()` — error but leave the `)` for `expect` below (clean recovery).
            TokKind::RParen => {
                self.diags.error("`link(\"name\")` needs a library-name string (e.g. `link(\"z\")`)", self.span());
                None
            }
            // Any other token — error and consume it so the `)` below still matches (no cascade).
            _ => {
                self.diags.error("`link(\"name\")` needs a library-name string (e.g. `link(\"z\")`)", self.span());
                if !self.at(&TokKind::Eof) {
                    self.bump();
                }
                None
            }
        };
        self.expect(&TokKind::RParen, "')'");
        name
    }

    /// One bodyless foreign signature `fn name(params) -> ret` (no generics, no body). Shares the
    /// parameter/return shape with [`Self::parse_fn`].
    fn parse_extern_sig(&mut self) -> Option<ExternSig> {
        let start = self.span();
        self.expect(&TokKind::Fn, "'fn'");
        let name = self.parse_ident("function name")?;
        self.expect(&TokKind::LParen, "'('");
        let mut params = Vec::new();
        while !self.at(&TokKind::RParen) && !self.at(&TokKind::Eof) {
            let pname = self.parse_ident("parameter name")?;
            self.expect(&TokKind::Colon, "':'");
            let ty = self.parse_type()?;
            params.push(Param { is_out: false, name: pname, ty });
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
        self.eat(&TokKind::End);
        let span = start.merge(self.prev_span());
        Some(ExternSig { name, params, ret, span })
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
            // A tuple destructuring `let`: `(a, b, ...) := expr`. Detected by lookahead (a
            // parenthesized name list followed by `:=`) so a parenthesized expression statement
            // stays unambiguous.
            if self.looks_like_destructure() {
                let s = self.parse_let_tuple()?;
                stmts.push(s);
                continue;
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
                self.expect_stmt_end();
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

    /// Lookahead: does the current `(` begin a tuple-destructuring pattern `(a, b, ...) :=`?
    /// A name list (each binder an identifier or `_`), arity ≥ 2, closed by `)` then `:=`.
    fn looks_like_destructure(&self) -> bool {
        if !self.at(&TokKind::LParen) {
            return false;
        }
        let mut i = 1;
        let mut count = 0;
        loop {
            if !matches!(self.peek_at(i), TokKind::Ident(_)) {
                return false;
            }
            count += 1;
            i += 1;
            if matches!(self.peek_at(i), TokKind::RParen) {
                i += 1;
                break;
            }
            if !matches!(self.peek_at(i), TokKind::Comma) {
                return false;
            }
            i += 1;
            // A trailing comma before `)`.
            if matches!(self.peek_at(i), TokKind::RParen) {
                i += 1;
                break;
            }
        }
        count >= 2 && matches!(self.peek_at(i), TokKind::ColonEq)
    }

    fn parse_let_tuple(&mut self) -> Option<Stmt> {
        let start = self.span();
        self.bump(); // '('
        let mut names = Vec::new();
        loop {
            let id = self.parse_ident("binding name or '_'")?;
            // `_` is the ignore binder (no local bound).
            names.push(if id.name == "_" { None } else { Some(id) });
            if self.eat(&TokKind::RParen) {
                break;
            }
            self.expect(&TokKind::Comma, "','");
            if self.eat(&TokKind::RParen) {
                break;
            }
        }
        self.expect(&TokKind::ColonEq, "':='");
        let init = self.parse_expr(0)?;
        self.expect_stmt_end();
        let span = start.merge(self.prev_span());
        Some(Stmt::LetTuple { names, init, span })
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
        self.expect_stmt_end();
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
        self.expect_stmt_end();
        Some(Stmt::Return(value))
    }

    // --- expressions (Pratt) ---

    /// Binary operator binding power (higher binds tighter) + how many tokens it spans. `None` = not
    /// a binary operator here. Precedence follows Go: shifts and `&` bind like `*` (5); `|` / `^`
    /// like `+` (4) — so every bitwise/shift operator binds tighter than comparison (3), avoiding the
    /// C `a & b == c` footgun.
    fn binop(&self) -> Option<(BinOp, u8, usize)> {
        // Two-token shifts: in expression position `<`/`>` are comparison-only (Align has no
        // turbofish), so an adjacent pair is unambiguously a shift. `>>` is deliberately *not* a
        // single token, so nested generic type arguments (`Pair<Pair<T>>`) still close — shifts
        // never occur in type position.
        match (self.peek(), self.peek_at(1)) {
            (TokKind::Lt, TokKind::Lt) => return Some((BinOp::Shl, 5, 2)),
            (TokKind::Gt, TokKind::Gt) => return Some((BinOp::Shr, 5, 2)),
            _ => {}
        }
        let (op, bp) = match self.peek() {
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
            TokKind::Pipe => (BinOp::BitOr, 4),
            TokKind::Caret => (BinOp::BitXor, 4),
            TokKind::Star => (BinOp::Mul, 5),
            TokKind::Slash => (BinOp::Div, 5),
            TokKind::Percent => (BinOp::Rem, 5),
            TokKind::Amp => (BinOp::BitAnd, 5),
            _ => return None,
        };
        Some((op, bp, 1))
    }

    /// Parse a `{...}` template hole's contents as a sub-expression. The hole source is
    /// re-lexed and parsed; token spans are offset to point into the template literal
    /// (`base = after the opening quote + the hole's offset in the content`). The mapping is
    /// exact when no escapes precede the hole (good enough for diagnostics). A parse failure
    /// yields a `Unit` placeholder so the surrounding template still produces an AST node.
    fn parse_hole_expr(&mut self, src: &str, str_span: Span, off: usize) -> Expr {
        let base = str_span.lo + 1 + off as u32;
        let remap = |s: Span| Span::new(s.file, s.lo + base, s.hi + base);
        // Lex into a private buffer so lexer-error spans (relative to the hole source) can be
        // offset into the template literal before merging into the real diagnostics.
        let mut hole_diags = Diagnostics::new();
        let tokens: Vec<Token> = align_lexer::tokenize(str_span.file, src, &mut hole_diags)
            .into_iter()
            .map(|t| Token { kind: t.kind, span: remap(t.span) })
            .collect();
        for d in hole_diags.iter() {
            self.diags.push(Diagnostic {
                severity: d.severity,
                message: d.message.clone(),
                span: d.span.map(remap),
            });
        }
        let mut sub = Parser { tokens, pos: 0, diags: self.diags, no_struct_literal: false, depth: 0 };
        let expr = sub.parse_expr(0);
        // The lexer appends an implicit `End` before `Eof`; skip it, then reject any
        // remaining tokens (e.g. `{x y}`): a hole must be exactly one expression.
        while matches!(sub.peek(), TokKind::End) {
            sub.bump();
        }
        if expr.is_some() && !matches!(sub.peek(), TokKind::Eof) {
            sub.diags.error("a template hole must be a single expression".to_string(), sub.span());
        }
        // On parse failure, point the placeholder at just the hole's contents.
        let hole_span = Span::new(str_span.file, base, base + src.len() as u32);
        expr.unwrap_or(Expr { kind: ExprKind::Unit, span: hole_span })
    }

    fn parse_expr(&mut self, min_bp: u8) -> Option<Expr> {
        self.depth += 1;
        if self.depth > MAX_EXPR_DEPTH {
            self.depth -= 1;
            self.diags.error("expression nests too deeply".to_string(), self.span());
            return None;
        }
        let r = self.parse_expr_inner(min_bp);
        self.depth -= 1;
        r
    }

    fn parse_expr_inner(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_cast()?;
        while let Some((op, bp, n)) = self.binop() {
            if bp < min_bp {
                break;
            }
            for _ in 0..n {
                self.bump();
            }
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

    /// Parse a sub-expression that sits inside a delimiter (`(...)`, `[...]`, call args, an array
    /// or struct-literal body). A `{` there can't begin a header block, so the `no_struct_literal`
    /// condition restriction is lifted for it (and restored afterwards).
    fn parse_delimited_expr(&mut self, min_bp: u8) -> Option<Expr> {
        let saved = std::mem::replace(&mut self.no_struct_literal, false);
        let e = self.parse_expr(min_bp);
        self.no_struct_literal = saved;
        e
    }

    /// `expr as T (as U)*` — explicit conversions, between unary prefix and the binary operators.
    /// `as` applies to the whole prefix expression (so `-x as i64` is `(-x) as i64`), and chains
    /// left-to-right (`x as i64 as f64`).
    fn parse_cast(&mut self) -> Option<Expr> {
        let mut e = self.parse_prefix()?;
        while self.at(&TokKind::As) {
            self.bump();
            let ty = self.parse_cast_type()?;
            let span = e.span.merge(ty.span());
            e = Expr { kind: ExprKind::Cast { expr: Box::new(e), ty }, span };
        }
        Some(e)
    }

    /// Parse a cast target type (`expr as T`). A cast target is always a concrete primitive scalar
    /// (int / float / char / `raw`), never generic — so this reads a bare (possibly dotted) type
    /// name and does **not** consume a following `<` as generic arguments. That keeps ordinary code
    /// like `x as u32 < 5` parseable (the `<` is the comparison, not the start of `u32<…>`).
    fn parse_cast_type(&mut self) -> Option<Type> {
        let path = self.parse_path();
        if path.segments.is_empty() {
            // `parse_path` → `parse_ident` already emitted "expected identifier" at this span;
            // just bail (don't double-report), matching `parse_type`.
            return None;
        }
        let span = path.span;
        Some(Type::Named { path, args: Vec::new(), span })
    }

    fn parse_prefix(&mut self) -> Option<Expr> {
        // A long unary chain (`----…x`) recurses here without going through `parse_expr`, so guard
        // depth here too (shared counter) — else it overflows the native stack and aborts.
        self.depth += 1;
        if self.depth > MAX_EXPR_DEPTH {
            self.depth -= 1;
            self.diags.error("expression nests too deeply".to_string(), self.span());
            return None;
        }
        let r = self.parse_prefix_inner();
        self.depth -= 1;
        r
    }

    fn parse_prefix_inner(&mut self) -> Option<Expr> {
        let start = self.span();
        let op = match self.peek() {
            TokKind::Minus => Some(UnOp::Neg),
            TokKind::Bang => Some(UnOp::Not),
            TokKind::Tilde => Some(UnOp::BitNot),
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
                    args.push(self.parse_delimited_expr(0)?);
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
            } else if self.at(&TokKind::Dot) && matches!(self.peek_at(1), TokKind::Int(_)) {
                // `recv.0` — positional tuple access.
                self.bump(); // '.'
                let ispan = self.span();
                let TokKind::Int(v) = self.bump().kind else { unreachable!() };
                let index = u32::try_from(v).unwrap_or(u32::MAX);
                let span = e.span.merge(ispan);
                e = Expr { kind: ExprKind::TupleIndex { recv: Box::new(e), index }, span };
            } else if self.at(&TokKind::LBracket) {
                self.bump(); // '['
                // `recv[a..b]` (range slice) vs `recv[i]` (element index). A leading `..` means the
                // start is omitted (`[..b]` / `[..]`); otherwise parse the first expr, and a `..`
                // after it makes this a slice (`[a..b]` / `[a..]`), else a plain index (`[i]`).
                if self.at(&TokKind::DotDot) {
                    self.bump(); // '..'
                    let end = if self.at(&TokKind::RBracket) { None } else { Some(Box::new(self.parse_delimited_expr(0)?)) };
                    self.expect(&TokKind::RBracket, "']'");
                    let span = e.span.merge(self.prev_span());
                    e = Expr { kind: ExprKind::SliceRange { recv: Box::new(e), start: None, end }, span };
                } else {
                    let first = self.parse_delimited_expr(0)?;
                    if self.at(&TokKind::DotDot) {
                        self.bump(); // '..'
                        let end = if self.at(&TokKind::RBracket) { None } else { Some(Box::new(self.parse_delimited_expr(0)?)) };
                        self.expect(&TokKind::RBracket, "']'");
                        let span = e.span.merge(self.prev_span());
                        e = Expr { kind: ExprKind::SliceRange { recv: Box::new(e), start: Some(Box::new(first)), end }, span };
                    } else {
                        self.expect(&TokKind::RBracket, "']'");
                        let span = e.span.merge(self.prev_span());
                        e = Expr { kind: ExprKind::Index { recv: Box::new(e), index: Box::new(first) }, span };
                    }
                }
            } else {
                break;
            }
        }
        Some(e)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let span = self.span();
        match self.peek() {
            TokKind::Int(v) => {
                let v = *v;
                self.bump();
                Some(Expr {
                    kind: ExprKind::Int(v),
                    span,
                })
            }
            TokKind::Float(v) => {
                let v = *v;
                self.bump();
                Some(Expr {
                    kind: ExprKind::Float(v),
                    span,
                })
            }
            TokKind::Char(v) => {
                let v = *v;
                self.bump();
                Some(Expr {
                    kind: ExprKind::Char(v),
                    span,
                })
            }
            TokKind::Str(_) => {
                let token = self.bump();
                let TokKind::Str(s) = token.kind else { unreachable!() };
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
                // `Name { field: ... }` / `mod.Name { field: ... }` is a struct literal.
                // Distinguish from a block following a bare name (e.g. an `if` condition) by the
                // `{ ident :` shape — no valid statement-block starts that way. The type name may
                // be a dotted path (`geom.Point`), so skip over `(. ident)*` before the brace.
                if !self.no_struct_literal
                    && let Some(segs) = self.struct_lit_path_len()
                {
                    return self.parse_struct_lit(segs);
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
                let content = if let TokKind::Str(_) = self.peek() {
                    let TokKind::Str(c) = self.bump().kind else { unreachable!() };
                    c
                } else {
                    self.diags.error("expected a string literal after `template`", str_span);
                    return None;
                };
                let parts = split_template(&content)
                    .into_iter()
                    .map(|rp| match rp {
                        RawPart::Text(s) => TemplatePart::Text(s),
                        RawPart::Hole { src, off } => {
                            TemplatePart::Hole(self.parse_hole_expr(&src, str_span, off))
                        }
                    })
                    .collect();
                let span = span.merge(self.prev_span());
                Some(Expr { kind: ExprKind::Template(parts), span })
            }
            TokKind::Fn => {
                // A lambda: `fn p0, p1 { ... }` — bare comma-separated parameter idents (types
                // inferred at the use site), then a block body. (Top-level `fn` is parsed at item
                // level, so a `fn` in expression position is always a lambda.)
                let start = self.span();
                self.bump();
                let mut params = Vec::new();
                if !matches!(self.peek(), TokKind::LBrace) {
                    loop {
                        let name = self.parse_ident("lambda parameter")?;
                        // Optional type annotation (`x: T`); required when the lambda is a value.
                        let ty = if self.eat(&TokKind::Colon) { Some(self.parse_type()?) } else { None };
                        params.push(LambdaParam { name, ty });
                        if !self.eat(&TokKind::Comma) {
                            break;
                        }
                    }
                }
                let body = self.parse_block()?;
                let span = start.merge(self.prev_span());
                Some(Expr { kind: ExprKind::Lambda { params, body }, span })
            }
            TokKind::If => self.parse_if(),
            TokKind::Arena => {
                let start = self.span();
                self.bump();
                let block = self.parse_block()?;
                let span = start.merge(self.prev_span());
                Some(Expr { kind: ExprKind::Arena(block), span })
            }
            TokKind::Unsafe => {
                let start = self.span();
                self.bump();
                let block = self.parse_block()?;
                let span = start.merge(self.prev_span());
                Some(Expr { kind: ExprKind::Unsafe(block), span })
            }
            TokKind::TaskGroup => {
                let start = self.span();
                self.bump();
                let block = self.parse_block()?;
                let span = start.merge(self.prev_span());
                Some(Expr { kind: ExprKind::TaskGroup(block), span })
            }
            TokKind::Match => self.parse_match(),
            TokKind::LParen => {
                self.bump();
                // `()` is the unit value; `(e)` is grouping; `(e0, e1, ...)` is a tuple.
                if self.at(&TokKind::RParen) {
                    self.bump();
                    let span = span.merge(self.prev_span());
                    return Some(Expr { kind: ExprKind::Unit, span });
                }
                let mut elems = vec![self.parse_delimited_expr(0)?];
                while self.eat(&TokKind::Comma) {
                    if self.at(&TokKind::RParen) {
                        break;
                    }
                    elems.push(self.parse_delimited_expr(0)?);
                }
                self.expect(&TokKind::RParen, "')'");
                let span = span.merge(self.prev_span());
                Some(if elems.len() == 1 {
                    elems.pop().expect("guarded: len == 1")
                } else {
                    Expr { kind: ExprKind::Tuple(elems), span }
                })
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
                    elems.push(self.parse_delimited_expr(0)?);
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

    /// Whether the tokens ahead form a struct literal `Path { ident : ...`, where `Path` is a
    /// dotted name (`Name` or `geom.Point`). Returns the path's segment count so `parse_struct_lit`
    /// knows how many `ident`-`.` pairs to consume; `None` if the shape does not match.
    fn struct_lit_path_len(&self) -> Option<usize> {
        // Path: ident (`.` ident)* — count segments, find the index just past the last one.
        let mut segs = 1usize;
        let mut i = 1; // index of the token after the leading ident
        while matches!(self.peek_at(i), TokKind::Dot) && matches!(self.peek_at(i + 1), TokKind::Ident(_)) {
            segs += 1;
            i += 2;
        }
        // Followed by `{ ident :` — the unambiguous struct-literal shape.
        if matches!(self.peek_at(i), TokKind::LBrace)
            && matches!(self.peek_at(i + 1), TokKind::Ident(_))
            && matches!(self.peek_at(i + 2), TokKind::Colon)
        {
            Some(segs)
        } else {
            None
        }
    }

    /// `Path { field: value, ... }`. Assumes [`struct_lit_path_len`] already matched; `segs` is the
    /// number of dotted segments in the type name.
    fn parse_struct_lit(&mut self, segs: usize) -> Option<Expr> {
        let start = self.span();
        let mut segments = Vec::with_capacity(segs);
        segments.push(self.parse_ident("type name")?);
        for _ in 1..segs {
            self.expect(&TokKind::Dot, "'.'");
            segments.push(self.parse_ident("type name")?);
        }
        let name = Path { span: start.merge(self.prev_span()), segments };
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
            let value = self.parse_delimited_expr(0)?;
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

    /// `match scrutinee { Variant => body, _ => body }` — arms are `pattern => expr`, separated by
    /// commas and/or newlines. The scrutinee parses like an `if` condition (a trailing `{` starts
    /// the arms, not a struct literal).
    fn parse_match(&mut self) -> Option<Expr> {
        let start = self.span();
        self.bump(); // match
        // The scrutinee parses like an `if` condition — a trailing `{` starts the arms, not a
        // struct literal (`match p { … }`, never `match (P { … })`-as-scrutinee without parens).
        // Restore the flag *before* the `?` so a scrutinee parse error can't leave it stuck on
        // (which would disable struct literals for the rest of the file → cascading errors).
        let saved = std::mem::replace(&mut self.no_struct_literal, true);
        let scrutinee = self.parse_expr(0);
        self.no_struct_literal = saved;
        let scrutinee = Box::new(scrutinee?);
        self.expect(&TokKind::LBrace, "'{'");
        let mut arms = Vec::new();
        loop {
            self.skip_ends();
            if self.at(&TokKind::RBrace) || self.at(&TokKind::Eof) {
                break;
            }
            let astart = self.span();
            let id = self.parse_ident("match pattern (a variant name or `_`)")?;
            let pattern = if id.name == "_" {
                MatchPattern::Wildcard(id.span)
            } else if self.at(&TokKind::Pipe) {
                // Or-pattern: `A | B | ...` — bare variant names, no bindings.
                let mut variants = vec![id];
                while self.eat(&TokKind::Pipe) {
                    self.skip_ends();
                    variants.push(self.parse_ident("a variant name in an or-pattern")?);
                }
                MatchPattern::Or { span: variants[0].span.merge(self.prev_span()), variants }
            } else {
                // Optional positional payload bindings: `Circle(r)`, `Rect(w, h)`.
                let mut bindings = Vec::new();
                if self.eat(&TokKind::LParen) {
                    loop {
                        self.skip_ends();
                        if self.at(&TokKind::RParen) || self.at(&TokKind::Eof) {
                            break;
                        }
                        bindings.push(self.parse_ident("a payload binding")?);
                        if !self.eat(&TokKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokKind::RParen, "')'");
                    if self.at(&TokKind::Pipe) {
                        self.diags.error(
                            "an or-pattern cannot bind a payload; list bare variant names (`A | B`) or use separate arms".to_string(),
                            self.span(),
                        );
                        // Recover: consume the rest of the (invalid) or-pattern tail so parsing
                        // resumes at `=>` rather than cascading into an "expected '=>'" error.
                        while self.eat(&TokKind::Pipe) {
                            self.skip_ends();
                            let _ = self.parse_ident("a variant name in an or-pattern");
                        }
                    }
                }
                MatchPattern::Variant { name: id, bindings }
            };
            self.expect(&TokKind::FatArrow, "'=>'");
            let body = Box::new(self.parse_expr(0)?);
            arms.push(MatchArm { pattern, body, span: astart.merge(self.prev_span()) });
            self.eat(&TokKind::Comma);
        }
        self.expect(&TokKind::RBrace, "'}'");
        let span = start.merge(self.prev_span());
        Some(Expr { kind: ExprKind::Match { scrutinee, arms }, span })
    }

    fn parse_if(&mut self) -> Option<Expr> {
        let start = self.span();
        self.bump(); // if
        // The `{` after the condition opens the `if` body, so a bare `Name { … }` in the condition
        // is not a struct literal (it must be parenthesized). Suppress struct-literal recognition
        // at the condition's top level; delimiters inside it lift the restriction.
        // Restore the flag *before* the `?` so a condition parse error can't leave it stuck on
        // (which would disable struct literals for the rest of the file → cascading errors).
        let saved = std::mem::replace(&mut self.no_struct_literal, true);
        let cond = self.parse_expr(0);
        self.no_struct_literal = saved;
        let cond = cond?;
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
        // A function type `fn(T, U) -> R` (a higher-order-function parameter).
        if self.at(&TokKind::Fn) {
            let start = self.span();
            self.bump();
            self.expect(&TokKind::LParen, "'('");
            let mut params = Vec::new();
            while !self.at(&TokKind::RParen) && !self.at(&TokKind::Eof) {
                params.push(self.parse_type()?);
                if !self.eat(&TokKind::Comma) {
                    break;
                }
            }
            self.expect(&TokKind::RParen, "')'");
            self.expect(&TokKind::Arrow, "'->'");
            let ret = Box::new(self.parse_type()?);
            let span = start.merge(self.prev_span());
            return Some(Type::Fn { params, ret, span });
        }
        // A parenthesized type: `()` = unit, `(T)` = grouping, `(T, U, ...)` = tuple.
        if self.at(&TokKind::LParen) {
            let start = self.span();
            self.bump();
            // The unit type `()` (a `Named` with the sentinel path "()").
            if self.eat(&TokKind::RParen) {
                let span = start.merge(self.prev_span());
                let seg = Ident { name: "()".to_string(), span };
                return Some(Type::Named { path: Path { segments: vec![seg], span }, args: Vec::new(), span });
            }
            let mut elems = vec![self.parse_type()?];
            while self.eat(&TokKind::Comma) {
                if self.at(&TokKind::RParen) {
                    break;
                }
                elems.push(self.parse_type()?);
            }
            self.expect(&TokKind::RParen, "')'");
            let span = start.merge(self.prev_span());
            // `(T)` is just grouping; `(T, U, ...)` is a tuple.
            return Some(if elems.len() == 1 {
                elems.pop().expect("guarded: len == 1")
            } else {
                Type::Tuple { elems, span }
            });
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
        Some(Type::Named { path, args, span })
    }

    fn parse_ident(&mut self, what: &str) -> Option<Ident> {
        let span = self.span();
        if let TokKind::Ident(_) = self.peek() {
            let TokKind::Ident(name) = self.bump().kind else { unreachable!() };
            Some(Ident { name, span })
        } else {
            self.diags.error(format!("expected {what}"), span);
            None
        }
    }

    /// Consume a weak keyword (one that appears as an `Ident`), like `out`.
    fn eat_ident_keyword(&mut self, kw: &str) -> bool {
        if let TokKind::Ident(name) = self.peek()
            && name == kw {
                self.bump();
                return true;
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
    fn slice_range_forms_parse() {
        // `a[i]` stays an Index; the four range shapes parse into SliceRange with the right
        // present/omitted bounds. Inspect the returned expression of each.
        let ret_expr = |src: &str| -> ExprKind {
            let (f, err) = parse(src);
            assert!(!err, "parse error in: {src}");
            let Item::Fn(fd) = &f.items[0] else { panic!() };
            let FnBody::Block(b) = &fd.body else { panic!() };
            let Stmt::Return(Some(e)) = &b.stmts[b.stmts.len() - 1] else { panic!() };
            e.kind.clone()
        };
        assert!(matches!(ret_expr("fn f() -> i64 { return a[2] }\n"), ExprKind::Index { .. }));
        match ret_expr("fn f() -> i64 { return a[1..5] }\n") {
            ExprKind::SliceRange { start: Some(_), end: Some(_), .. } => {}
            other => panic!("a[1..5] → {other:?}"),
        }
        match ret_expr("fn f() -> i64 { return a[1..] }\n") {
            ExprKind::SliceRange { start: Some(_), end: None, .. } => {}
            other => panic!("a[1..] → {other:?}"),
        }
        match ret_expr("fn f() -> i64 { return a[..5] }\n") {
            ExprKind::SliceRange { start: None, end: Some(_), .. } => {}
            other => panic!("a[..5] → {other:?}"),
        }
        match ret_expr("fn f() -> i64 { return a[..] }\n") {
            ExprKind::SliceRange { start: None, end: None, .. } => {}
            other => panic!("a[..] → {other:?}"),
        }
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
        use crate::RawPart;
        // A non-empty `{...}` is a hole (its contents are parsed as an expression later).
        let p = split_template("a {x + 1} b");
        assert!(matches!(p.as_slice(), [RawPart::Text(_), RawPart::Hole { .. }, RawPart::Text(_)]));
        // An unmatched `{` and an empty `{}` stay literal text.
        for s in ["unmatched {", "empty {}"] {
            let parts = split_template(s);
            assert!(
                parts.iter().all(|p| matches!(p, RawPart::Text(_))),
                "expected all-literal for {s:?}, got {parts:?}"
            );
        }
    }

    #[test]
    fn template_hole_parses_expression() {
        // `{a + b}` parses as a Binary expression hole, not just a bare name.
        let (f, err) = parse("fn main() -> i32 {\n  m := template \"{a + b}\"\n  return 0\n}\n");
        assert!(!err, "expression hole should parse");
        let Item::Fn(fd) = &f.items[0] else { panic!() };
        let FnBody::Block(b) = &fd.body else { panic!() };
        let Stmt::Let { init, .. } = &b.stmts[0] else { panic!() };
        let ExprKind::Template(parts) = &init.kind else { panic!("expected template") };
        assert!(
            matches!(parts.as_slice(), [TemplatePart::Hole(e)] if matches!(e.kind, ExprKind::Binary { .. })),
            "hole should be a binary expression, got {parts:?}"
        );
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
    fn statements_on_one_line_require_a_separator() {
        // 2-3: two statements crammed onto one line without a `;` (or newline) must be rejected —
        // the Go-style "a newline terminates a statement" rule.
        let (_f, err) = parse("fn main() -> i32 { x := 1 return x }\n");
        assert!(err, "`x := 1 return x` (no separator) must error");
        // A `;` separator is fine.
        let (_f, err) = parse("fn main() -> i32 { x := 1; return x }\n");
        assert!(!err, "`;`-separated statements parse");
        // A newline separator is fine.
        let (_f, err) = parse("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        assert!(!err, "newline-separated statements parse");
        // The last statement needs no trailing separator before the closing brace.
        let (_f, err) = parse("fn main() -> i32 { return 0 }\n");
        assert!(!err, "a single statement before the closing brace needs no separator");
    }

    #[test]
    fn name_block_in_if_condition_is_not_a_struct_literal() {
        // `if p { ... }` must parse `p` as the condition, not `p { ... }` as a literal.
        let (_f, err) = parse("fn f(p: bool) -> i32 {\n  if p { return 1 }\n  return 0\n}\n");
        assert!(!err);
    }

    #[test]
    fn type_annotated_let_at_if_head_is_not_a_struct_literal() {
        // `if flag { x: i32 := 5 … }` — the body's leading `ident :` used to be misread as a
        // struct-literal field, breaking the condition. It must parse without error.
        let (_f, err) = parse("fn f(flag: bool) -> i32 {\n  if flag { x: i32 := 5\n    return x }\n  return 0\n}\n");
        assert!(!err);
    }

    #[test]
    fn struct_literal_in_condition_call_args_still_parses() {
        // The `no_struct_literal` restriction is lifted inside delimiters: a struct literal passed
        // as a call argument in a condition is still a struct literal.
        let (_f, err) = parse("P { a: i32 }\nfn g(p: P) -> bool = true\nfn f() -> i32 {\n  if g(P { a: 1 }) { return 1 }\n  return 0\n}\n");
        assert!(!err);
    }

    #[test]
    fn deeply_nested_input_errors_instead_of_overflowing_stack() {
        // 2-4: pathological nesting used to overflow the native stack and abort the process. It must
        // now report a bounded diagnostic and return. Run on an 8 MB stack (the compiler parses on
        // the main thread, whose default stack is that large) — the default *test* thread stack is
        // smaller, and the point is that the guard bounds recursion, not that it fits 2 MB.
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(|| {
                let parens = format!("fn main() -> i32 {{ return {}1{} }}\n", "(".repeat(50_000), ")".repeat(50_000));
                let (_f, err) = parse(&parens);
                assert!(err, "deep parens should report an error, not crash");
                let unary = format!("fn main() -> i32 {{ return {}1 }}\n", "-".repeat(100_000));
                let (_f, err) = parse(&unary);
                assert!(err, "deep unary chain should report an error, not crash");
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn condition_parse_error_does_not_leak_no_struct_literal_flag() {
        // Regression (gemini #272): if the condition itself fails to parse, the `no_struct_literal`
        // flag must still be restored — otherwise every later struct literal in the file is
        // misparsed. After a broken `if` condition, a subsequent struct literal must still be a
        // `StructLit` in the AST.
        let src = "fn bad() -> i32 {\n  if ) { return 1 }\n  return 0\n}\nP { a: i32 }\nfn good() -> i32 {\n  p := P { a: 5 }\n  return p.a\n}\n";
        let (f, _err) = parse(src); // `bad` reports an error; that's expected.
        let good = f.items.iter().find_map(|it| match it {
            Item::Fn(fd) if fd.name.name == "good" => Some(fd),
            _ => None,
        }).expect("`good` function should still parse");
        let FnBody::Block(b) = &good.body else { panic!() };
        let Stmt::Let { init, .. } = &b.stmts[0] else { panic!("first stmt of `good` should be a let") };
        assert!(
            matches!(init.kind, ExprKind::StructLit { .. }),
            "after a broken condition, `P {{ a: 5 }}` must still parse as a struct literal, got {:?}",
            init.kind
        );
    }
}
