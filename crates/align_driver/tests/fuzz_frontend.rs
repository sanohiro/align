//! Fuzzing (dependency-free, deterministic — no nightly / cargo-fuzz needed): the compiler front
//! end (lexer → parser → sema) must **never panic** on any input — it may only emit diagnostics.
//! This is the "the compiler diagnoses, never crashes" invariant (align-self-review Gate 3), the
//! class the 2026-07-02 audit kept hitting (a 50k-deep-nesting stack overflow, an `unreachable!` on
//! a malformed node). Inputs are seeded so any failure is reproducible from its printed seed.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// SplitMix64 — a tiny, allocation-free, reproducible PRNG (no `rand` dependency).
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

/// The lexeme vocabulary a fuzzed program is assembled from — real Align tokens, so generated input
/// gets *past the lexer into the parser* (where the panics hide), plus literals and every delimiter.
const TOKENS: &[&str] = &[
    "fn", "return", "mut", "if", "else", "match", "arena", "unsafe", "extern", "task_group", "spawn",
    "wait", "import", "module", "pub", "as", "true", "false", "template", "heap", "box", "soa",
    "->", "=>", ":=", "==", "!=", "<=", ">=", "&&", "||", "<<", ">>",
    "(", ")", "{", "}", "[", "]", "<", ">", ",", ".", ":", ";", "=", "+", "-", "*", "/", "%",
    "&", "|", "^", "~", "!", "?", "..",
    "x", "y", "f", "T", "U", "p", "main", "User", "Color", "Red",
    "i8", "i32", "i64", "u32", "u64", "f32", "f64", "str", "string", "bool", "char",
    "array", "slice", "Option", "Result", "Error", "print", "json", "decode",
    "0", "1", "42", "-5", "1.5", "0xff", "1e9", "\"s\"", "\"n={x}\"", "'a'", "'\\n'",
];

/// Build a random token soup of `len` lexemes; occasionally splice a raw byte to stress the lexer's
/// UTF-8 / unexpected-character paths.
fn gen_soup(rng: &mut Rng, len: usize) -> String {
    let mut s = String::new();
    for _ in 0..len {
        if rng.below(25) == 0 {
            // A random Unicode scalar — often non-ASCII, so the lexer's multi-byte UTF-8 decode +
            // non-ASCII "unexpected character" paths get exercised (not just 7-bit ASCII).
            let cp = rng.below(0x2000) as u32;
            s.push(char::from_u32(cp).unwrap_or('a'));
        } else {
            s.push_str(TOKENS[rng.below(TOKENS.len())]);
            s.push(if rng.below(4) == 0 { '\n' } else { ' ' });
        }
    }
    s
}

/// Run the pure front end (no module IO): lexer → parser → sema on a single source string.
fn frontend(src: &str) {
    let mut diags = align_diag::Diagnostics::new();
    let mut sm = align_span::SourceMap::new();
    let fid = sm.add_file("fuzz", src);
    let toks = align_lexer::tokenize(fid, src, &mut diags);
    let ast = align_parser::parse_file(toks, &mut diags);
    let _ = align_sema::check_file(&ast, &mut diags);
}

fn assert_no_panic(seed: u64, src: &str) {
    let owned = src.to_string();
    let r = catch_unwind(AssertUnwindSafe(|| frontend(&owned)));
    assert!(r.is_ok(), "front-end panicked on fuzz seed {seed}:\n---\n{src}\n---");
}

#[test]
fn frontend_never_panics_on_token_soup() {
    // Bare token soup at top level — exercises item/type/decl parsing + sema on malformed programs.
    for seed in 0..12_000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(1));
        let len = 1 + rng.below(50);
        let src = gen_soup(&mut rng, len);
        assert_no_panic(seed, &src);
    }
}

#[test]
fn frontend_never_panics_on_token_soup_in_a_function_body() {
    // Wrap the soup in a function so it reaches statement / expression parsing + the flow analyses
    // (move / escape / effect) and finalize — where the deeper `unreachable!`s live.
    for seed in 0..12_000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x1234_5678_9ABC_DEF1).wrapping_add(7));
        let len = 1 + rng.below(40);
        let src = format!("fn main() -> i32 {{\n{}\n}}\n", gen_soup(&mut rng, len));
        assert_no_panic(seed, &src);
    }
}
