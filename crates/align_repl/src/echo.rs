//! The three-candidate echo procedure (`docs/impl/22-repl-plan.md` §3.4).
//!
//! The design principle is negative: the REPL does not synthesize bindings and therefore never has
//! to reason about whether an echo consumed the user's value. `print(E)` borrows, a bare place
//! expression does not move, and the one synthetic binding (`_ := (E)`, case 3) is disclosed to the
//! user rather than analysed away.

use crate::entry::EntryKind;

/// What the REPL tells the user about the entry's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Echo {
    /// Nothing to say: a declaration, a binding, or a statement with no value.
    None,
    /// The value printed itself (§3.4 case 1).
    Printed,
    /// Not printable: the rendered type only. No value was touched.
    TypeOnly { rendered: String },
    /// §3.4 case 3 — a bare fallible expression, bound with `_` so it is handled. The `note:` line
    /// is part of the contract, because binding a Result-typed *place expression* moves it.
    ResultBound { rendered: String },
}

impl Echo {
    /// The one-line note that must accompany a `ResultBound` echo.
    pub const RESULT_NOTE: &'static str = "note: a Result must be handled — `?`, `match`, or bind it with a name";

    pub fn render(&self) -> Option<String> {
        match self {
            Echo::None | Echo::Printed => None,
            Echo::TypeOnly { rendered } => Some(format!("<{rendered}>")),
            Echo::ResultBound { rendered } => Some(format!("<{rendered}>\n{}", Echo::RESULT_NOTE)),
        }
    }
}

/// The entry kinds §3.4 can choose for a region-4 entry, in candidate order.
pub(crate) const CANDIDATE_ORDER: [EntryKind; 3] = [EntryKind::Printed, EntryKind::Statement, EntryKind::ResultBound];

/// Whether every error in a rendered diagnostic block is the hard `unhandled Result` rule — the
/// only failure of candidate S that case 3 is allowed to answer.
pub(crate) fn only_unhandled_result(rendered: &str) -> bool {
    let mut saw = false;
    for line in rendered.lines() {
        let Some(message) = crate::diagnostic_error_message(line) else {
            continue;
        };
        saw = true;
        if !message.contains("unhandled `Result`") {
            return false;
        }
    }
    saw
}

/// Render the type of the region-4 entry at `index`, for the non-printable echo.
///
/// Indexed, not "the last value in `main`": an entry that REPLACES an earlier one sits in the
/// middle of the body, and reading the body's tail would echo a different entry's value — which
/// the first end-to-end run of this REPL did, reporting `<()>` for `x := 9` because a later
/// `print(…)` was the trailing statement.
///
/// The index is exact because a region-4 entry emits exactly one statement: it was accepted only
/// because it parsed as a single statement (§3.3 steps 2–4), and `Entry::emit_for` wraps rather
/// than splits. The HIR lookup is bounds-checked so malformed input fails closed with no echo.
///
/// Reads the checked HIR rather than a diagnostic string, and renders through the compiler's own
/// `ty_display`, so the REPL and a diagnostic can never spell the same type differently.
/// `type_params` is `&[]`: a checked `main` is never a generic template.
pub(crate) fn entry_type(program: &align_sema::Program, index: usize) -> Option<String> {
    let main = program.fns.iter().find(|f| f.name == "main")?;
    let align_sema::hir::Stmt::Expr(e) = main.body.stmts.get(index)? else {
        // A binding, an assignment, a `return` — a statement with no value to report.
        return None;
    };
    if e.ty == align_sema::Ty::Unit {
        // `print(…)`, a call returning nothing: the statement ran, and there is nothing to say
        // about a value that does not exist. Echoing `<()>` is noise on every `print` a user types.
        return None;
    }
    render_type(program, e.ty)
}

/// Type of §3.4 case D's synthetic `_ := (E)`. Unlike an ordinary statement echo, this reads the
/// binding initializer: `_` creates no user-visible name, but HIR still retains the expression and
/// its complete `Result<T, E>` type.
pub(crate) fn result_bound_type(program: &align_sema::Program, index: usize) -> Option<String> {
    let main = program.fns.iter().find(|f| f.name == "main")?;
    let ty = match main.body.stmts.get(index)? {
        align_sema::hir::Stmt::Let { init, .. } | align_sema::hir::Stmt::LetTuple { init, .. } => init.ty,
        align_sema::hir::Stmt::Expr(expr) => expr.ty,
        _ => return None,
    };
    render_type(program, ty)
}

fn render_type(program: &align_sema::Program, ty: align_sema::Ty) -> Option<String> {
    if ty == align_sema::Ty::Unit {
        return None;
    }
    Some(align_sema::ty_display(
        ty,
        &program.structs,
        &program.enums,
        &program.tagged_types,
        &program.tuples,
        &[],
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn unhandled_result_classifier_ignores_source_excerpts() {
        assert!(super::only_unhandled_result(
            "error: unhandled `Result` value\n  1 | print(\"error: source text\")"
        ));
        assert!(super::only_unhandled_result(
            "/tmp/session.align:6:3: error: unhandled `Result` value"
        ));
        assert!(!super::only_unhandled_result(
            "error: unhandled `Result` value\nerror: undefined name: 'missing'"
        ));
    }
}
