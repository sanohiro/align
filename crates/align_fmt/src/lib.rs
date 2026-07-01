//! The official Align source formatter (`alignc fmt`).
//!
//! Policy (`draft.md` §4/§16, settled): normalize **only meaningless variation** — inter-token
//! spacing, `;` placement, alignment, blank-line runs — and **preserve the author's one-line ↔
//! multi-line choice**. It is NOT a reflowing formatter; it never decides line breaks.
//!
//! Mechanism (settled, see `open-questions.md` "Formatter"): a **hybrid token-reprint + AST-assist**.
//! The lexer drops comments and every non-statement newline, so neither tokens nor the AST can
//! round-trip trivia — but spans retain the full source. So we walk the significant tokens, emit
//! each token's text **verbatim from its source span** (literals/escapes/radix preserved), decide
//! only the whitespace between tokens, and recover comments / newlines / blank lines / `;` from the
//! gap `src[prev.hi .. cur.lo]`. The AST is consulted only to disambiguate the two context-sensitive
//! spacing cases: a `<`/`>` that is a generic type-argument bracket (no surrounding space) vs a
//! comparison (spaced), and a unary `-`/`~`/`!` (no trailing space) vs a binary operator.
//!
//! Safety: format only parse-clean input, and verify the formatted output re-tokenizes to the
//! identical significant-token sequence — otherwise pass the source through unchanged. So a
//! formatter bug can never change a program's meaning; at worst it declines to format.

use std::collections::HashSet;

use align_ast::*;
use align_diag::Diagnostics;
use align_lexer::{tokenize, Token};
use align_span::FileId;

const INDENT: &str = "  "; // two spaces per level (matches the examples)

/// Format Align source. Returns the formatted text, or `None` if the input does not parse cleanly
/// or the result would not round-trip (the caller should then leave the source unchanged).
pub fn format_source(file: FileId, src: &str) -> Option<String> {
    let mut diags = Diagnostics::new();
    let parse_tokens = tokenize(file, src, &mut diags);
    let ast = align_parser::parse_file(parse_tokens, &mut diags);
    if diags.has_errors() {
        return None;
    }
    let ann = Annotations::collect(&ast, src);
    // Re-lex (parse consumed the first vector) for the emit pass.
    let tokens = tokenize(file, src, &mut Diagnostics::new());
    let out = Formatter::new(src, &ann).run(&tokens);

    // Safety net, in two layers. (1) The formatted text must carry the exact same significant
    // tokens, in order — catches a dropped/added/garbled token. (2) It must re-parse cleanly —
    // catches a structural corruption the token check misses (e.g. losing a statement-separating
    // `;`, which leaves the token sequence unchanged but changes the parse). If either fails, decline
    // to format rather than risk changing the program's meaning.
    let mut out_diags = Diagnostics::new();
    let out_tokens = tokenize(file, &out, &mut out_diags);
    if sig_texts(&tokens, src) != sig_texts(&out_tokens, &out) {
        return None;
    }
    align_parser::parse_file(out_tokens, &mut out_diags);
    if out_diags.has_errors() {
        return None;
    }
    Some(out)
}

fn is_skipped(t: &Token) -> bool {
    matches!(t.kind, align_lexer::TokKind::End | align_lexer::TokKind::Eof)
}

fn tok_text<'s>(src: &'s str, t: &Token) -> &'s str {
    // Spans come from tokenizing this exact `src`, so they are in bounds — but clamp defensively so
    // the formatter can never panic (a public entry point), e.g. on a synthetic/zero-width span.
    let lo = (t.span.lo as usize).min(src.len());
    let hi = (t.span.hi as usize).min(src.len());
    if lo <= hi {
        &src[lo..hi]
    } else {
        ""
    }
}

/// The significant tokens' source texts, in order — the meaning-bearing fingerprint of a program
/// (whitespace, `;`, and comments are all absent here, so they don't affect equality).
fn sig_texts(tokens: &[Token], src: &str) -> Vec<String> {
    tokens.iter().filter(|t| !is_skipped(t)).map(|t| tok_text(src, t).to_string()).collect()
}

// ---------------------------------------------------------------------------
// AST-derived annotations (the "assist" half)
// ---------------------------------------------------------------------------

/// Byte-offset information the token reprinter cannot derive locally.
struct Annotations {
    /// `[lo, hi)` ranges that are type-argument context — a `<`/`>`/`,` whose offset falls inside one
    /// is a generic bracket (no surrounding space), not a comparison.
    type_ranges: Vec<(u32, u32)>,
    /// Offsets of unary prefix operators (`-`/`~`/`!`) — a `-` here takes no trailing space.
    unary_ops: HashSet<u32>,
}

impl Annotations {
    fn collect(file: &File, src: &str) -> Annotations {
        let mut a = Annotations { type_ranges: Vec::new(), unary_ops: HashSet::new() };
        for item in &file.items {
            a.visit_item(item, src);
        }
        a
    }

    fn in_type_range(&self, off: u32) -> bool {
        self.type_ranges.iter().any(|&(lo, hi)| off >= lo && off < hi)
    }

    fn visit_item(&mut self, item: &Item, src: &str) {
        match item {
            Item::Fn(d) => {
                self.visit_generics(&d.type_params, src);
                for p in &d.params {
                    self.visit_type(&p.ty);
                }
                if let Some(r) = &d.ret {
                    self.visit_type(r);
                }
                match &d.body {
                    FnBody::Block(b) => self.visit_block(b),
                    FnBody::Expr(e) => self.visit_expr(e),
                }
            }
            Item::Struct(d) => {
                self.visit_generics(&d.type_params, src);
                for f in &d.fields {
                    self.visit_type(&f.ty);
                }
            }
            Item::Enum(d) => {
                self.visit_generics(&d.type_params, src);
                for v in &d.variants {
                    for t in &v.payload {
                        self.visit_type(t);
                    }
                }
            }
            Item::Const(d) => {
                if let Some(t) = &d.ty {
                    self.visit_type(t);
                }
                self.visit_expr(&d.value);
            }
        }
    }

    /// A decl generic-parameter list `<T, U: Ord>`. The `TypeParam`s have no span of their own, so
    /// take the span from the first param name to the last param's bound (or name), then widen over
    /// whitespace to swallow the enclosing `<` and `>` characters.
    fn visit_generics(&mut self, params: &[TypeParam], src: &str) {
        let (Some(first), Some(last)) = (params.first(), params.last()) else {
            return;
        };
        let lo = first.name.span.lo;
        let hi = last.bound.as_ref().map(|b| b.span.hi).unwrap_or(last.name.span.hi);
        let b = src.as_bytes();
        // Clamp defensively (these offsets are in-bounds for `src`, but never index past the end).
        // widen left to the `<`
        let mut l = (lo as usize).min(b.len());
        while l > 0 && b[l - 1].is_ascii_whitespace() {
            l -= 1;
        }
        if l > 0 && b[l - 1] == b'<' {
            l -= 1;
        }
        // widen right to the `>`
        let mut r = (hi as usize).min(b.len());
        while r < b.len() && b[r].is_ascii_whitespace() {
            r += 1;
        }
        if r < b.len() && b[r] == b'>' {
            r += 1;
        }
        self.type_ranges.push((l as u32, r as u32));
    }

    fn visit_type(&mut self, ty: &Type) {
        match ty {
            Type::Named { args, span, .. } => {
                if !args.is_empty() {
                    // The Named span covers `Path<args>`, including the `<` and `>`.
                    self.type_ranges.push((span.lo, span.hi));
                    for a in args {
                        self.visit_type(a);
                    }
                }
            }
            Type::Tuple { elems, .. } => {
                for e in elems {
                    self.visit_type(e);
                }
            }
            Type::Fn { params, ret, .. } => {
                for p in params {
                    self.visit_type(p);
                }
                self.visit_type(ret);
            }
        }
    }

    fn visit_block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.visit_stmt(s);
        }
        if let Some(t) = &b.tail {
            self.visit_expr(t);
        }
    }

    fn visit_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { ty, init, .. } => {
                if let Some(t) = ty {
                    self.visit_type(t);
                }
                self.visit_expr(init);
            }
            Stmt::LetTuple { init, .. } => self.visit_expr(init),
            Stmt::Assign { place, value } => {
                self.visit_expr(place);
                self.visit_expr(value);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) => self.visit_expr(e),
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Unary { expr, .. } => {
                self.unary_ops.insert(e.span.lo);
                self.visit_expr(expr);
            }
            ExprKind::Cast { expr, ty } => {
                self.visit_expr(expr);
                self.visit_type(ty);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            ExprKind::Call { callee, args } => {
                self.visit_expr(callee);
                for a in args {
                    self.visit_expr(a);
                }
            }
            ExprKind::FieldAccess { recv, .. } => self.visit_expr(recv),
            ExprKind::If { cond, then, els } => {
                self.visit_expr(cond);
                self.visit_block(then);
                if let Some(e) = els {
                    self.visit_expr(e);
                }
            }
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) | ExprKind::Unsafe(b) => self.visit_block(b),
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.visit_expr(&f.value);
                }
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.visit_expr(opt);
                self.visit_expr(fallback);
            }
            ExprKind::Try(e) => self.visit_expr(e),
            ExprKind::ArrayLit(es) | ExprKind::Tuple(es) => {
                for e in es {
                    self.visit_expr(e);
                }
            }
            ExprKind::Index { recv, index } => {
                self.visit_expr(recv);
                self.visit_expr(index);
            }
            ExprKind::SliceRange { recv, start, end } => {
                self.visit_expr(recv);
                if let Some(e) = start {
                    self.visit_expr(e);
                }
                if let Some(e) = end {
                    self.visit_expr(e);
                }
            }
            ExprKind::Lambda { params, body } => {
                for p in params {
                    if let Some(t) = &p.ty {
                        self.visit_type(t);
                    }
                }
                self.visit_block(body);
            }
            ExprKind::Match { scrutinee, arms } => {
                self.visit_expr(scrutinee);
                for arm in arms {
                    self.visit_expr(&arm.body);
                }
            }
            ExprKind::TupleIndex { recv, .. } => self.visit_expr(recv),
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(e) = p {
                        self.visit_expr(e);
                    }
                }
            }
            // Leaves: no nested exprs/types.
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Path(_)
            | ExprKind::FieldShorthand(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Trivia recovered from an inter-token gap
// ---------------------------------------------------------------------------

enum Triv {
    Newline,
    Semicolon,
    Comment(String),
}

/// Scan an inter-token gap into its meaningful trivia: newlines, `;`, and `//` line comments.
/// Whitespace carries no formatting information and is dropped. A `;` is kept because it is
/// load-bearing when cramming statements on one line (`x := 1; y := 2`) — only a `;` immediately
/// before a newline is redundant, and that is decided at emit time, not here.
fn scan_gap(gap: &str) -> Vec<Triv> {
    let b = gap.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                out.push(Triv::Newline);
                i += 1;
            }
            b';' => {
                out.push(Triv::Semicolon);
                i += 1;
            }
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => {
                let start = i;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                out.push(Triv::Comment(gap[start..i].trim_end().to_string()));
            }
            _ => i += 1, // whitespace, `\r`
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The emit pass
// ---------------------------------------------------------------------------

struct Formatter<'a> {
    src: &'a str,
    ann: &'a Annotations,
    out: String,
    depth: usize,
}

impl<'a> Formatter<'a> {
    fn new(src: &'a str, ann: &'a Annotations) -> Formatter<'a> {
        Formatter { src, ann, out: String::new(), depth: 0 }
    }

    fn run(mut self, tokens: &[Token]) -> String {
        let sig: Vec<&Token> = tokens.iter().filter(|t| !is_skipped(t)).collect();
        let mut prev_hi = 0u32;
        let mut prev: Option<&Token> = None;
        for t in &sig {
            // Defensive clamp (spans are in-bounds for this `src`; a formatter must never panic).
            let lo = (prev_hi as usize).min(self.src.len());
            let hi = (t.span.lo as usize).min(self.src.len());
            let gap = if lo <= hi { &self.src[lo..hi] } else { "" };
            self.emit(gap, prev, t);
            prev = Some(t);
            prev_hi = t.span.hi;
        }
        // Trailing trivia (EOF comments). `self.src` is `&'a str` (Copy), so this slice doesn't
        // borrow `self`.
        let src = self.src;
        self.emit_trailing(&src[(prev_hi as usize).min(src.len())..]);
        while self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push('\n');
        self.out
    }

    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    /// Break to a new line; `nl` ≥ 2 means a blank line was present — keep exactly one. Leading
    /// blank lines (at the very start of the file) are dropped.
    fn break_line(&mut self, nl: usize) {
        if self.out.is_empty() {
            return;
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        if nl >= 2 && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn indent_line(&mut self, depth: usize, continuation: bool) {
        let n = depth + if continuation { 1 } else { 0 };
        for _ in 0..n {
            self.out.push_str(INDENT);
        }
    }

    fn emit(&mut self, gap: &str, prev: Option<&Token>, t: &Token) {
        let mut pending_nl = 0usize;
        let mut had_semi = false;
        for ev in scan_gap(gap) {
            match ev {
                Triv::Newline => pending_nl += 1,
                Triv::Semicolon => had_semi = true,
                Triv::Comment(c) => {
                    if pending_nl == 0 && !self.at_line_start() {
                        // Trailing comment on the current line.
                        self.out.push(' ');
                        self.out.push_str(&c);
                    } else {
                        self.break_line(pending_nl);
                        pending_nl = 0;
                        self.indent_line(self.depth, false);
                        self.out.push_str(&c);
                    }
                }
            }
        }

        let closing = matches!(
            t.kind,
            align_lexer::TokKind::RBrace | align_lexer::TokKind::RParen | align_lexer::TokKind::RBracket
        );
        if closing {
            self.depth = self.depth.saturating_sub(1);
        }

        if pending_nl > 0 || self.at_line_start() {
            // A `;` immediately before a newline is a redundant terminator → dropped (we break here).
            self.break_line(pending_nl);
            let cont = self.is_continuation(t);
            self.indent_line(self.depth, cont);
        } else if had_semi {
            // Cramming: `;` separates statements on one line — load-bearing, keep it.
            self.out.push_str("; ");
        } else if let Some(p) = prev {
            self.out.push_str(self.sep(p, t));
        }
        self.out.push_str(tok_text(self.src, t));

        let opening = matches!(
            t.kind,
            align_lexer::TokKind::LBrace | align_lexer::TokKind::LParen | align_lexer::TokKind::LBracket
        );
        if opening {
            self.depth += 1;
        }
    }

    fn emit_trailing(&mut self, tail: &str) {
        let mut pending_nl = 0usize;
        for ev in scan_gap(tail) {
            match ev {
                Triv::Newline => pending_nl += 1,
                // A `;` after the last token is always redundant (nothing follows) → dropped.
                Triv::Semicolon => {}
                Triv::Comment(c) => {
                    if pending_nl == 0 && !self.at_line_start() {
                        self.out.push(' ');
                        self.out.push_str(&c);
                    } else {
                        self.break_line(pending_nl);
                        pending_nl = 0;
                        self.indent_line(self.depth, false);
                        self.out.push_str(&c);
                    }
                }
            }
        }
    }

    /// A line that starts with `.` or a binary operator continues the previous line (settled
    /// multi-line-chain rule) → one extra indent level. A leading unary `-`/`!`/`~` does not.
    fn is_continuation(&self, t: &Token) -> bool {
        use align_lexer::TokKind::*;
        match t.kind {
            Dot | DotDot => true,
            Plus | Minus | Star | Slash | Percent | EqEq | NotEq | Lt | Le | Gt | Ge | AndAnd
            | OrOr | Amp | Pipe | Caret => !self.ann.unary_ops.contains(&t.span.lo),
            _ => false,
        }
    }

    /// Canonical whitespace between two same-line tokens: `""` (hug) or `" "`.
    fn sep(&self, prev: &Token, cur: &Token) -> &'static str {
        use align_lexer::TokKind::*;
        let p = &prev.kind;
        let c = &cur.kind;

        // After a unary prefix operator: no space (`-x`, `!ok`, `~bits`).
        if self.ann.unary_ops.contains(&prev.span.lo) && matches!(p, Minus | Bang | Tilde) {
            return "";
        }

        // `<<` / `>>` are each lexed as two `Lt`/`Gt` tokens (so nested generics like `Pair<Pair<T>>`
        // still close). When they were adjacent in the source they form one shift / close operator —
        // keep them hugged so `1 << 3` does not become `1 < < 3`.
        if matches!((p, c), (Lt, Lt) | (Gt, Gt)) && prev.span.hi == cur.span.lo {
            return "";
        }

        // Generic type brackets hug their *inner* side: no space before either bracket (`Pair<`,
        // `T>`), none after an opening `<` (`<T`). After a closing `>` we fall through to normal
        // spacing, so `Pair<T> {` and `Pair<T>)` come out right (a nested `>>` still hugs because
        // the following `>` is itself a type bracket).
        let cur_type_angle = matches!(c, Lt | Gt) && self.ann.in_type_range(cur.span.lo);
        if cur_type_angle {
            return "";
        }
        if matches!(p, Lt) && self.ann.in_type_range(prev.span.lo) {
            return "";
        }

        // No space before these.
        if matches!(c, Comma | Colon | Question | Dot | DotDot) {
            return "";
        }
        // No space after these.
        if matches!(p, Dot | DotDot) {
            return "";
        }
        // No space just inside ( and [.
        if matches!(p, LParen | LBracket) || matches!(c, RParen | RBracket) {
            return "";
        }
        // A `(` / `[` directly after a value is a call / index (no space); otherwise a group / literal.
        // A closing generic `>` counts as a value end so a decl's param list hugs (`fn f<T>(...)`).
        let prev_type_close = matches!(p, Gt) && self.ann.in_type_range(prev.span.lo);
        let value_end = matches!(p, Ident(_) | Int(_) | Float(_) | Str(_) | Char(_) | RParen | RBracket | RBrace | Question | True | False)
            || prev_type_close;
        if matches!(c, LParen | LBracket) && value_end {
            return "";
        }
        // Braces: space around a non-empty `{ ... }`, but `{}` hugs.
        if matches!(c, LBrace) {
            return " ";
        }
        if matches!(p, LBrace) {
            return if matches!(c, RBrace) { "" } else { " " };
        }
        if matches!(c, RBrace) {
            return if matches!(p, LBrace) { "" } else { " " };
        }

        " "
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format_source(0, src).unwrap_or_else(|| panic!("did not format:\n{src}"))
    }

    #[test]
    fn idempotent_on_messy_spacing() {
        let messy = "fn main()->i32{\n  x:=1+2*3\n  return  x\n}\n";
        let once = fmt(messy);
        let twice = fmt(&once);
        assert_eq!(once, twice, "formatting is not idempotent");
        assert!(once.contains("x := 1 + 2 * 3"), "spacing not normalized:\n{once}");
        assert!(once.contains("fn main() -> i32 {"), "header not normalized:\n{once}");
    }

    #[test]
    fn preserves_line_break_choice() {
        // One-line and multi-line forms are both fixed points (modulo spacing) — not reflowed.
        let one = fmt("fn f() -> i32 { return 1 }\n");
        assert_eq!(one, "fn f() -> i32 { return 1 }\n");
        let multi = fmt("fn f() -> i32 {\n  return 1\n}\n");
        assert_eq!(multi, "fn f() -> i32 {\n  return 1\n}\n");
    }

    #[test]
    fn preserves_comments() {
        let src = "// header\nfn main() -> i32 {\n  return 0 // done\n}\n";
        let out = fmt(src);
        assert!(out.contains("// header"), "lost header comment:\n{out}");
        assert!(out.contains("return 0 // done"), "lost trailing comment:\n{out}");
    }

    #[test]
    fn drops_redundant_semicolons_and_collapses_blanks() {
        let src = "fn main() -> i32 {\n  x := 1;\n\n\n  return x;\n}\n";
        let out = fmt(src);
        assert!(!out.contains(';'), "redundant ; not dropped:\n{out}");
        assert!(!out.contains("\n\n\n"), "blank run not collapsed:\n{out}");
        assert!(out.contains("x := 1\n"), "statement mangled:\n{out}");
    }

    #[test]
    fn preserves_cramming_semicolons() {
        // A `;` that separates statements on one line is load-bearing — it must survive (only a `;`
        // immediately before a newline is redundant).
        let out = fmt("fn main() -> i32 {\n  x := 1; y := 2\n  return x + y\n}\n");
        assert!(out.contains("x := 1; y := 2"), "cramming ; dropped (corrupts code):\n{out}");
    }

    #[test]
    fn shifts_and_generics_hug_correctly() {
        // `<<`/`>>` are two `Lt`/`Gt` tokens but must not split into `< <`; generic brackets hug,
        // but a comparison `<` is spaced.
        let out = fmt("fn f<T>(p: Pair<T>) -> i32 {\n  x := 1 << 3 >> 1\n  return x < 9 as i32\n}\n");
        assert!(out.contains("1 << 3 >> 1"), "shift split:\n{out}");
        assert!(out.contains("fn f<T>(p: Pair<T>)"), "generics not hugged:\n{out}");
        assert!(out.contains("x < 9"), "comparison not spaced:\n{out}");
    }

    #[test]
    fn refuses_to_format_broken_code() {
        assert!(format_source(0, "fn main( -> {{{\n").is_none());
    }
}
