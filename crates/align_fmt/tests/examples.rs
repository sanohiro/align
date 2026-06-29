//! Format every `examples/*.align` and assert the three correctness properties on real code:
//!   1. it formats (parses cleanly → `Some`),
//!   2. formatting is idempotent (`fmt(fmt(x)) == fmt(x)`), and
//!   3. it is meaning-preserving — the formatted text carries the identical significant-token
//!      sequence as the original (the same property `format_source` enforces internally, re-checked
//!      here at the suite level so a regression in that guard is caught).

use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Significant-token texts (whitespace / `;` / comments excluded) — the meaning fingerprint.
fn sig_tokens(src: &str) -> Vec<String> {
    let mut d = align_diag::Diagnostics::new();
    align_lexer::tokenize(0, src, &mut d)
        .iter()
        .filter(|t| !matches!(t.kind, align_lexer::TokKind::End | align_lexer::TokKind::Eof))
        .map(|t| src[t.span.lo as usize..t.span.hi as usize].to_string())
        .collect()
}

#[test]
fn formats_all_examples_idempotently_and_meaning_preserving() {
    let dir = examples_dir();
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("read examples dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("align") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let once = align_fmt::format_source(0, &src)
            .unwrap_or_else(|| panic!("{name}: a clean example failed to format"));
        let twice = align_fmt::format_source(0, &once)
            .unwrap_or_else(|| panic!("{name}: formatted output failed to re-format"));
        assert_eq!(once, twice, "{name}: formatting is not idempotent");
        assert_eq!(
            sig_tokens(&src),
            sig_tokens(&once),
            "{name}: formatting changed the significant tokens (meaning not preserved)"
        );
        count += 1;
    }
    assert!(count >= 40, "expected to exercise many examples, only saw {count}");
}
