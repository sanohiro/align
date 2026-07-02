//! Formatter property tests. `align_fmt::format_source` must (1) never panic on any input,
//! (2) be **idempotent** on real programs — `fmt(fmt(x)) == fmt(x)` (a stable fixed point, like
//! gofmt), and (3) produce output that still **parses without new errors**. The audit flagged the
//! missing "reparse / idempotency" property tests for the formatter.

use std::panic::{catch_unwind, AssertUnwindSafe};

fn fmt(src: &str) -> Option<String> {
    align_fmt::format_source(0, src)
}

/// Parse `src` and return the concatenated diagnostic messages if it fails to parse cleanly (so a
/// property failure can show *why* the reformatted output no longer parses), or `None` if clean.
fn parse_errors(src: &str) -> Option<String> {
    let mut diags = align_diag::Diagnostics::new();
    let mut sm = align_span::SourceMap::new();
    let fid = sm.add_file("f", src);
    let toks = align_lexer::tokenize(fid, src, &mut diags);
    let _ = align_parser::parse_file(toks, &mut diags);
    if diags.has_errors() {
        Some(diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "))
    } else {
        None
    }
}

/// Every checked-in example is a real, valid program — the formatter must be idempotent on each and
/// preserve parseability. (`examples/modules/` are multi-file; the top-level ones are single-file.)
fn examples() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "align"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn formatter_is_idempotent_and_parse_preserving_on_examples() {
    let paths = examples();
    // Guard against a vacuous pass (wrong path / no examples found).
    assert!(paths.len() >= 50, "expected the examples corpus, found {} files", paths.len());
    for path in paths {
        let src = std::fs::read_to_string(&path).unwrap();
        let name = path.display();
        // Every checked-in (single-file) example is a valid program, so the formatter must format
        // it — a `None` here is a formatter bug, not something to skip.
        let once = fmt(&src).unwrap_or_else(|| panic!("formatter declined to format the valid example {name}"));
        // Formatting the formatted output must be a no-op (a stable fixed point).
        let twice = fmt(&once).unwrap_or_else(|| panic!("fmt of formatted {name} returned None"));
        assert_eq!(once, twice, "formatter is not idempotent on {name}");
        // The formatted output must still parse without introducing errors.
        if let Some(errs) = parse_errors(&once) {
            panic!("formatted {name} no longer parses cleanly: {errs}\n--- formatted ---\n{once}");
        }
    }
}

// --- fuzz: the formatter must never panic on arbitrary input (shares the SplitMix64 soup) ---

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const TOKENS: &[&str] = &[
    "fn", "return", "mut", "if", "else", "match", "arena", "pub", "as", "true", "template",
    "->", "=>", ":=", "==", "&&", "||", "(", ")", "{", "}", "[", "]", "<", ">", ",", ".", ":", ";",
    "=", "+", "-", "*", "/", "?", "..", "x", "main", "i32", "array", "Option", "0", "42", "\"s\"", " ", "\n",
];

#[test]
fn formatter_never_panics_on_fuzzed_input() {
    for seed in 0..10_000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(3));
        let len = 1 + rng.below(40);
        let mut src = String::new();
        for _ in 0..len {
            src.push_str(TOKENS[rng.below(TOKENS.len())]);
            src.push(' ');
        }
        let owned = src.clone();
        let r = catch_unwind(AssertUnwindSafe(|| {
            // If it formats, formatting again must not panic either (exercise the fixed-point path).
            if let Some(once) = fmt(&owned) {
                let _ = fmt(&once);
            }
        }));
        assert!(r.is_ok(), "formatter panicked on fuzz seed {seed}:\n---\n{src}\n---");
    }
}
