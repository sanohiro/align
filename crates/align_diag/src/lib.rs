//! Common foundation for diagnostics (errors/warnings)
//! (`docs/impl/01-pipeline.md`, cross-cutting crate).
//! Each stage continues as far as possible on failure, accumulating diagnostics.

use align_span::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            span: Some(span),
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            span: Some(span),
        }
    }
}

/// Diagnostics accumulated over a single compilation.
#[derive(Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Diagnostics {
        Diagnostics { items: Vec::new() }
    }

    pub fn push(&mut self, diag: Diagnostic) {
        self.items.push(diag);
    }

    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.push(Diagnostic::error(message, span));
    }

    pub fn has_errors(&self) -> bool {
        self.items
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// The number of **error** diagnostics so far (warnings excluded) — a checkpoint for gating a
    /// follow-up check on "did this subtree already report an error", without being tripped by a
    /// warning emitted in the same subtree.
    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The number of diagnostics so far — a checkpoint for a speculative check (see [`Self::truncate`]).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Roll the diagnostics back to a checkpoint from [`Self::len`], discarding any emitted since —
    /// used to undo a speculative type-check whose errors would otherwise duplicate a later pass.
    pub fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }
}
