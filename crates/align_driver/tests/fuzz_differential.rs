//! Differential fuzzing: generate a **valid** Align program *together with the value it must
//! compute* (a reference oracle evaluated in Rust with matching semantics — wrapping i64 add/sub/mul,
//! truncate-toward-zero div/rem, signed comparisons), compile + run it, and assert the process exit
//! code equals the oracle. This drives whole programs through sema → MIR → LLVM → native and catches
//! **miscompiles** (a wrong result), the class the token-soup fuzzers can't reach (they rarely
//! produce a valid, runnable program). Seeded, so any mismatch is reproducible from its seed.

mod common;
use common::*;

/// SplitMix64 — reproducible, dependency-free PRNG.
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

/// Generate an `i64` expression string paired with the value Align must compute for it. Every
/// operation mirrors Align's defined semantics exactly so the oracle is authoritative:
/// `+ - *` wrap two's-complement, `/ %` truncate toward zero (divisor forced non-zero), comparisons
/// are signed, `if` picks a branch. Bounded by `depth`; leaves are 0..9 literals.
fn gen_expr(rng: &mut Rng, depth: u32) -> (String, i64) {
    if depth == 0 || rng.below(3) == 0 {
        let v = rng.below(10) as i64;
        return (v.to_string(), v);
    }
    match rng.below(5) {
        0 => {
            let (l, lv) = gen_expr(rng, depth - 1);
            let (r, rv) = gen_expr(rng, depth - 1);
            let (op, val) = match rng.below(3) {
                0 => ("+", lv.wrapping_add(rv)),
                1 => ("-", lv.wrapping_sub(rv)),
                _ => ("*", lv.wrapping_mul(rv)),
            };
            (format!("({l} {op} {r})"), val)
        }
        1 => {
            // Division / remainder with a forced non-zero divisor (Align aborts on div-by-zero).
            let (l, lv) = gen_expr(rng, depth - 1);
            let d = 1 + rng.below(9) as i64;
            if rng.below(2) == 0 {
                (format!("({l} / {d})"), lv.wrapping_div(d))
            } else {
                (format!("({l} % {d})"), lv.wrapping_rem(d))
            }
        }
        2 => {
            let (a, av) = gen_expr(rng, depth - 1);
            let (b, bv) = gen_expr(rng, depth - 1);
            let (t, tv) = gen_expr(rng, depth - 1);
            let (e, ev) = gen_expr(rng, depth - 1);
            let (op, cond) = match rng.below(6) {
                0 => ("<", av < bv),
                1 => ("<=", av <= bv),
                2 => (">", av > bv),
                3 => (">=", av >= bv),
                4 => ("==", av == bv),
                _ => ("!=", av != bv),
            };
            (format!("if {a} {op} {b} {{ {t} }} else {{ {e} }}"), if cond { tv } else { ev })
        }
        3 => {
            // Short-circuit `&&` / `||` combined into a branch value (both sides are comparisons).
            let (a, av) = gen_expr(rng, depth - 1);
            let (b, bv) = gen_expr(rng, depth - 1);
            let (t, tv) = gen_expr(rng, depth - 1);
            let (e, ev) = gen_expr(rng, depth - 1);
            let (op, cond) = if rng.below(2) == 0 {
                ("&&", (av < bv) && (bv < av.wrapping_add(3)))
            } else {
                ("||", (av > bv) || (av == bv))
            };
            let cond_src = if op == "&&" {
                format!("{a} < {b} && {b} < {a} + 3")
            } else {
                format!("{a} > {b} || {a} == {b}")
            };
            (format!("if {cond_src} {{ {t} }} else {{ {e} }}"), if cond { tv } else { ev })
        }
        _ => {
            // `-x` (unary neg on a signed value).
            let (x, xv) = gen_expr(rng, depth - 1);
            (format!("(0 - {x})"), 0i64.wrapping_sub(xv))
        }
    }
}

#[test]
fn generated_programs_compute_the_oracle_value() {
    if !backend_available() {
        return;
    }
    // Each iteration compiles + runs a native binary (~ms), so keep the count modest.
    for seed in 0..200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(11));
        let (expr, oracle) = gen_expr(&mut rng, 4);
        // `main` returns the value as `i32`. Unix truncates the process exit status to its low byte
        // (unsigned); Windows preserves the full 32-bit value.
        let expected = if cfg!(windows) { oracle as i32 } else { (oracle as i32 as u8) as i32 };
        let src = format!("fn main() -> i32 {{\n  r := {expr}\n  return r as i32\n}}\n");
        let out = build_and_run(&format!("diff-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {oracle}), got {code}\n--- program ---\n{src}"
        );
    }
}
