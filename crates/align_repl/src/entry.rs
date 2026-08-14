//! Entry classification, continuation, and the candidate name set
//! (`docs/impl/22-repl-plan.md` §3.3, §3.6, §9.1).

use align_diag::Diagnostics;
use align_lexer::TokKind;
use align_span::SourceMap;

/// Which of the four emission regions (§3.2) an entry lives in. Fixed at classification and never
/// changed: replacement (§3.5) only ever matches within one region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Import,
    Const,
    Decl,
    Main,
}

/// What the entry contributes, and (for a value entry) how §3.4 wrapped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Import,
    Decl,
    Const,
    /// Emitted verbatim (a binding, an assignment, or a non-printable expression).
    Statement,
    /// Emitted as `print(<text>)` — §3.4 case 1.
    Printed,
    /// Emitted as `_ := (<text>)` — §3.4 case 3, the one synthetic binding.
    ResultBound,
}

/// One accepted entry in the living program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Monotonically increasing and never reused (§3.7); removal leaves a visible gap.
    pub ordinal: u32,
    pub region: Region,
    pub kind: EntryKind,
    /// Exactly what the user typed.
    pub text: String,
    /// What is spliced into the program: `text`, `print(text)`, or `_ := (text)`. Nothing else —
    /// `emitted_forms_are_exhaustive` asserts that structurally.
    pub emitted: String,
    /// The names this entry owns in its region (§3.6).
    pub names: Vec<String>,
    /// Entries created by one multi-item paste share a group so `:undo` reverses the paste (§3.7.1).
    pub paste_group: Option<u32>,
}

impl Entry {
    /// The one place an entry's emitted text is derived from its kind. Keeping this a function of
    /// `(kind, text)` is what makes the exhaustiveness assertion in the owner suite meaningful.
    pub(crate) fn emit_for(kind: EntryKind, text: &str) -> String {
        match kind {
            EntryKind::Printed => format!("print({text})"),
            EntryKind::ResultBound => format!("_ := ({text})"),
            EntryKind::Import | EntryKind::Decl | EntryKind::Const | EntryKind::Statement => text.to_string(),
        }
    }
}

/// The lex-only triage of §3.0 stage 0 and the classification inputs of §3.3 steps 1–3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// No significant token: a blank line or comments only. Stage 0 makes this a no-op.
    Empty,
    /// `import` / `fn` / `extern` / `resource` first: unambiguously a declaration (step 1).
    KeywordDecl,
    /// Anything else; steps 2–4 decide.
    Other,
}

/// Tokenize `text` in isolation, discarding diagnostics, and report its §3.3 shape plus the facts
/// §9.1 needs. `resource` is contextual in the grammar, so it is matched as an identifier.
pub(crate) fn shape_of(text: &str) -> Shape {
    let mut sm = SourceMap::new();
    let file = sm.add_file("<repl-probe>", text.to_string());
    let mut diags = Diagnostics::new();
    let toks = align_lexer::tokenize(file, text, &mut diags);
    let first = toks.iter().find(|t| !matches!(t.kind, TokKind::End | TokKind::Eof));
    match first.map(|t| &t.kind) {
        None => Shape::Empty,
        Some(TokKind::Import | TokKind::Fn | TokKind::Extern) => Shape::KeywordDecl,
        Some(TokKind::Ident(name)) if name == "resource" => Shape::KeywordDecl,
        Some(_) => Shape::Other,
    }
}

/// Whether the accumulated entry is incomplete and the REPL should keep reading (§9.1).
///
/// Deliberately only two conditions: an unbalanced bracket, and an unterminated string or character
/// literal. The lexer's own leading-`.`/binary-operator continuation is private, so a method chain
/// broken across lines is supported only inside brackets — a limitation `:help` states rather than
/// approximates with a heuristic here.
pub(crate) fn needs_more(text: &str) -> bool {
    let mut sm = SourceMap::new();
    let file = sm.add_file("<repl-probe>", text.to_string());
    let mut diags = Diagnostics::new();
    let toks = align_lexer::tokenize(file, text, &mut diags);
    let mut depth: i32 = 0;
    for t in &toks {
        match t.kind {
            // `<`/`>` are NOT counted: they are ambiguous with comparison and generics.
            TokKind::LBrace | TokKind::LParen | TokKind::LBracket => depth += 1,
            TokKind::RBrace | TokKind::RParen | TokKind::RBracket => depth -= 1,
            _ => {}
        }
    }
    if depth > 0 {
        return true;
    }
    diags.iter().any(|d| {
        d.message.contains("unterminated string literal") || d.message.contains("unterminated character literal")
    })
}

/// Parse `text` on its own as a file-scope item list. `None` when it does not parse there.
pub(crate) fn parse_as_file(text: &str) -> Option<align_ast::File> {
    let mut sm = SourceMap::new();
    let file = sm.add_file("<repl-probe>", text.to_string());
    let mut diags = Diagnostics::new();
    let toks = align_lexer::tokenize(file, text, &mut diags);
    let parsed = align_parser::parse_file(toks, &mut diags);
    if diags.has_errors() || (parsed.imports.is_empty() && parsed.items.is_empty()) {
        return None;
    }
    Some(parsed)
}

/// Parse `text` as a statement by parsing a probe program that contains it as `main`'s only
/// statement. Returns whether it parses; sema is not run.
pub(crate) fn parses_as_statement(text: &str) -> bool {
    let src = format!("fn main() -> Result<(), Error> {{\n{text}\nreturn Ok(())\n}}\n");
    let mut sm = SourceMap::new();
    let file = sm.add_file("<repl-probe>", src.clone());
    let mut diags = Diagnostics::new();
    let toks = align_lexer::tokenize(file, &src, &mut diags);
    let _ = align_parser::parse_file(toks, &mut diags);
    !diags.has_errors()
}

/// The names a DECL/CONST entry declares at file scope, read from its own AST.
///
/// Region 2 has no HIR presence at all — a top-level constant is folded to a literal at every use
/// and never reaches `align_sema::Program` — so `:const` names can only come from here. Region 3
/// names are cross-checked against the HIR delta, which is what catches a compiler-generated name.
pub(crate) fn declared_names(file: &align_ast::File) -> Vec<String> {
    let mut names = Vec::new();
    for item in &file.items {
        names.extend(item_names(item));
    }
    names
}

/// Names contributed by one file-scope item. An extern block owns every literal C symbol in it.
pub(crate) fn item_names(item: &align_ast::Item) -> Vec<String> {
    match item {
        align_ast::Item::Fn(d) => vec![d.name.name.clone()],
        align_ast::Item::Struct(d) => vec![d.name.name.clone()],
        align_ast::Item::Enum(d) => vec![d.name.name.clone()],
        align_ast::Item::Resource(d) => vec![d.name.name.clone()],
        align_ast::Item::Const(d) => vec![d.name.name.clone()],
        align_ast::Item::Extern(b) => b.fns.iter().map(|f| f.name.name.clone()).collect(),
    }
}

/// Exact source text covered by one item's parser span. Mixed pastes use this to create one entry
/// per item without reformatting the user's declaration.
pub(crate) fn item_text<'a>(source: &'a str, item: &align_ast::Item) -> &'a str {
    let span = match item {
        align_ast::Item::Fn(d) => d.span,
        align_ast::Item::Struct(d) => d.span,
        align_ast::Item::Enum(d) => d.span,
        align_ast::Item::Resource(d) => d.span,
        align_ast::Item::Const(d) => d.span,
        align_ast::Item::Extern(d) => d.span,
    };
    let (Ok(lo), Ok(hi)) = (usize::try_from(span.lo), usize::try_from(span.hi)) else {
        return "";
    };
    source.get(lo..hi).unwrap_or("").trim()
}

/// Render an import path the way §3.7.1 compares them for deduplication.
pub(crate) fn import_path(path: &align_ast::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

/// The binding names introduced at the ENTRY's own top statement level (§3.6's second filter).
///
/// Required because HIR flattens every local of a function into one `Fn::locals`: without this an
/// `arena { p := … }` entry would appear to own `p`, and a later `p := 9` — which the compiler
/// accepts, because `p` is block-scoped — would wrongly displace the arena entry.
///
/// Computed from the parser's binding nodes in a probe `main`, so annotation and initializer
/// identifiers cannot be mistaken for binders. Malformed text contributes no trusted names; its
/// real candidate check reports the syntax diagnostics.
pub(crate) fn top_level_binding_names(text: &str) -> Vec<String> {
    let src = format!("fn main() -> Result<(), Error> {{\n{text}\nreturn Ok(())\n}}\n");
    let mut sm = SourceMap::new();
    let file = sm.add_file("<repl-probe>", src.clone());
    let mut diags = Diagnostics::new();
    let toks = align_lexer::tokenize(file, &src, &mut diags);
    let parsed = align_parser::parse_file(toks, &mut diags);
    if diags.has_errors() {
        return Vec::new();
    }
    let mut names = Vec::new();
    let Some(main) = parsed.items.iter().find_map(|item| match item {
        align_ast::Item::Fn(function) if function.name.name == "main" => Some(function),
        _ => None,
    }) else {
        return names;
    };
    let align_ast::FnBody::Block(body) = &main.body else {
        return names;
    };
    for statement in &body.stmts {
        match statement {
            align_ast::Stmt::Let { name, .. } if name.name != "_" => names.push(name.name.clone()),
            align_ast::Stmt::LetTuple { names: tuple, .. } => {
                names.extend(
                    tuple
                        .iter()
                        .flatten()
                        .filter(|name| name.name != "_")
                        .map(|name| name.name.clone()),
                );
            }
            _ => {}
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{Shape, needs_more, parses_as_statement, shape_of, top_level_binding_names};

    #[test]
    fn classification_lexical_front_door_is_total() {
        for text in ["", "   ", "// comment", "// comment\n"] {
            assert_eq!(shape_of(text), Shape::Empty, "{text:?}");
        }
        for text in [
            "import core.math",
            "fn f() -> i64 = 1",
            "extern \"C\" fn abs(x: i32) -> i32",
            "resource Handle = drop_handle",
        ] {
            assert_eq!(shape_of(text), Shape::KeywordDecl, "{text:?}");
        }
        for text in ["x := 1", "Point { x: i64 }", "print(1)"] {
            assert_eq!(shape_of(text), Shape::Other, "{text:?}");
        }
    }

    #[test]
    fn continuation_counts_only_brackets_and_unterminated_literals() {
        assert!(needs_more("if true {"));
        assert!(needs_more("(1 + 2"));
        assert!(needs_more("[1, 2"));
        assert!(needs_more("\"unterminated"));
        assert!(!needs_more("1 < 2"));
        assert!(!needs_more("if true { 1 }"));
    }

    #[test]
    fn top_level_name_filter_excludes_types_ignored_names_and_nested_bindings() {
        let cases = [
            ("x := 1", vec!["x"]),
            ("mut x := 1", vec!["x"]),
            ("typed: i64 := 1", vec!["typed"]),
            ("(a, b) := (1, 2)", vec!["a", "b"]),
            ("(a, _) := (1, 2)", vec!["a"]),
            ("_ := (1)", vec![]),
            ("arena { nested := 1; print(nested) }", vec![]),
            ("x = 2", vec![]),
            ("p.field = 2", vec![]),
            ("broken: i64 :=", vec![]),
        ];
        for (text, expected) in cases {
            assert_eq!(top_level_binding_names(text), expected, "{text}");
        }
        assert!(parses_as_statement("print(1)"));
        assert!(parses_as_statement("x := 1"));
    }
}
