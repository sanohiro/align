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
            // Division / remainder. The divisor is forced non-zero (Align aborts on div-by-zero,
            // which the oracle doesn't model) but may be negative, incl. `-1` — that exercises the
            // signed `INT_MIN / -1` wrap guard. `wrapping_div`/`wrapping_rem` model it exactly
            // (`x / -1 == -x` wrapping at MIN, `x % -1 == 0`).
            let (l, lv) = gen_expr(rng, depth - 1);
            let mag = 1 + rng.below(9) as i64;
            let d = if rng.below(4) == 0 { -mag } else { mag };
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
                ("||", av >= bv)
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

// --- typed, multi-width variant: exercises every integer width's wrapping + cross-width casts ---

#[derive(Clone, Copy, PartialEq)]
enum ITy {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}
const TYPES: [ITy; 8] = [ITy::I8, ITy::I16, ITy::I32, ITy::I64, ITy::U8, ITy::U16, ITy::U32, ITy::U64];
impl ITy {
    fn name(self) -> &'static str {
        match self {
            ITy::I8 => "i8",
            ITy::I16 => "i16",
            ITy::I32 => "i32",
            ITy::I64 => "i64",
            ITy::U8 => "u8",
            ITy::U16 => "u16",
            ITy::U32 => "u32",
            ITy::U64 => "u64",
        }
    }
    fn bits(self) -> u32 {
        match self {
            ITy::I8 | ITy::U8 => 8,
            ITy::I16 | ITy::U16 => 16,
            ITy::I32 | ITy::U32 => 32,
            ITy::I64 | ITy::U64 => 64,
        }
    }
    fn signed(self) -> bool {
        matches!(self, ITy::I8 | ITy::I16 | ITy::I32 | ITy::I64)
    }
}

/// Normalize a computed value into `t`'s representable range — the single operation that models
/// Align's *both* arithmetic wrapping (take the low `bits` two's-complement) *and* integer casts
/// (`S as T` = reinterpret the source value's bits in `T`, sign-/zero-extending per the source, which
/// falls out of passing the source's true numeric value here). Verified against the compiler
/// (`u8: 3 - 10 == 249`, `i8: 3 - 10 == -7`).
fn wrap(v: i128, t: ITy) -> i128 {
    let w = t.bits();
    let mask = (1u128 << w) - 1;
    let bits = (v as u128) & mask;
    if t.signed() && (bits >> (w - 1)) & 1 == 1 {
        (bits as i128) - (1i128 << w)
    } else {
        bits as i128
    }
}

/// Generate an expression *of type `t`* (all operands typed `t` so Align never coerces), paired with
/// its normalized oracle value. Leaves are 0..9 literals or an in-scope variable of type `t`; the
/// cast arm reinterprets any in-scope variable (of any width) into `t`.
fn gen_typed(rng: &mut Rng, depth: u32, t: ITy, vars: &[(String, ITy, i128)]) -> (String, i128) {
    let same: Vec<&(String, ITy, i128)> = vars.iter().filter(|v| v.1 == t).collect();
    if depth == 0 || rng.below(3) == 0 {
        if !same.is_empty() && rng.below(2) == 0 {
            let v = same[rng.below(same.len())];
            return (v.0.clone(), v.2);
        }
        let n = rng.below(10) as i128;
        return (n.to_string(), wrap(n, t)); // literal infers `t` from the binding annotation
    }
    match rng.below(5) {
        0 => {
            let (l, lv) = gen_typed(rng, depth - 1, t, vars);
            let (r, rv) = gen_typed(rng, depth - 1, t, vars);
            let (op, val) = match rng.below(3) {
                0 => ("+", lv.wrapping_add(rv)),
                1 => ("-", lv.wrapping_sub(rv)),
                _ => ("*", lv.wrapping_mul(rv)),
            };
            (format!("({l} {op} {r})"), wrap(val, t))
        }
        1 => {
            let (l, lv) = gen_typed(rng, depth - 1, t, vars);
            let mag = 1 + rng.below(9) as i128; // literal divisor infers `t`; forced non-zero
            // A signed type may also divide by a negative literal (incl. `-1` → the per-width
            // `INT_MIN / -1` wrap guard); an unsigned type rejects a negative literal.
            let d = if t.signed() && rng.below(4) == 0 { -mag } else { mag };
            if rng.below(2) == 0 {
                (format!("({l} / {d})"), wrap(lv.wrapping_div(d), t))
            } else {
                (format!("({l} % {d})"), wrap(lv.wrapping_rem(d), t))
            }
        }
        2 => {
            let (a, av) = gen_typed(rng, depth - 1, t, vars);
            let (b, bv) = gen_typed(rng, depth - 1, t, vars);
            let (tb, tv) = gen_typed(rng, depth - 1, t, vars);
            let (eb, ev) = gen_typed(rng, depth - 1, t, vars);
            let (op, cond) = match rng.below(6) {
                0 => ("<", av < bv),
                1 => ("<=", av <= bv),
                2 => (">", av > bv),
                3 => (">=", av >= bv),
                4 => ("==", av == bv),
                _ => ("!=", av != bv),
            };
            (format!("if {a} {op} {b} {{ {tb} }} else {{ {eb} }}"), if cond { tv } else { ev })
        }
        3 if !vars.is_empty() => {
            // Reinterpret any in-scope variable (of any width) into `t` — exercises trunc / sext / zext.
            let v = &vars[rng.below(vars.len())];
            (format!("({} as {})", v.0, t.name()), wrap(v.2, t))
        }
        _ => {
            let n = rng.below(10) as i128;
            (n.to_string(), wrap(n, t))
        }
    }
}

#[test]
fn typed_multiwidth_programs_compute_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(101));
        let stmts = 2 + rng.below(4); // 2..5 typed let-bindings
        let mut vars: Vec<(String, ITy, i128)> = Vec::new();
        let mut body = String::new();
        for i in 0..stmts {
            let t = TYPES[rng.below(TYPES.len())];
            let (expr, val) = gen_typed(&mut rng, 3, t, &vars);
            body.push_str(&format!("  v{i}: {} := {}\n", t.name(), expr));
            vars.push((format!("v{i}"), t, val));
        }
        let last = stmts - 1;
        let final_val = wrap(vars[last].2, ITy::I32); // `return vN as i32`
        body.push_str(&format!("  return v{last} as i32\n"));
        let src = format!("fn main() -> i32 {{\n{body}}}\n");
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("difft-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {}), got {code}\n--- program ---\n{src}",
            vars[last].2
        );
    }
}

// --- aggregate variant: struct field read-back + fixed-array indexing (the audit's miscompile
// class — an array index that returned garbage). Values are 0..9 literals; the oracle is the exact
// field / element stored. ---

#[test]
fn aggregates_compute_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0xA076_1D64_78BD_642F).wrapping_add(3));
        let (src, oracle) = if rng.below(2) == 0 {
            // Struct: build with concrete field values, then read *every* field back and sum them.
            // Mixed field widths force the compiler's descending-alignment field reordering; summing
            // all fields exercises every physical slot's mapped GEP in one program (a single stale
            // logical→physical index would corrupt the sum). Values are 0..9 (representable in every
            // width), so each field reads back exactly what was stored and the sum stays small.
            let nf = 2 + rng.below(3); // 2..4 fields
            let mut fields = String::new();
            let mut inits = String::new();
            let mut reads = String::new();
            let mut sum = 0i128;
            for f in 0..nf {
                let ty = TYPES[rng.below(TYPES.len())];
                let v = rng.below(10) as i128;
                if f > 0 {
                    fields.push_str(", ");
                    inits.push_str(", ");
                    reads.push_str(" + ");
                }
                fields.push_str(&format!("f{f}: {}", ty.name()));
                inits.push_str(&format!("f{f}: {v}"));
                reads.push_str(&format!("(s.f{f} as i32)"));
                sum += v; // 0..9 each, so no width wrap; the i32 sum wraps below
            }
            let src = format!(
                "S {{ {fields} }}\nfn main() -> i32 {{\n  s := S {{ {inits} }}\n  return {reads}\n}}\n"
            );
            (src, wrap(sum, ITy::I32))
        } else {
            // Fixed array of default-typed (i64) elements, indexed by a constant.
            let n = 2 + rng.below(4); // 2..5 elements
            let vals: Vec<i128> = (0..n).map(|_| rng.below(10) as i128).collect();
            let idx = rng.below(n);
            let elems: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
            let src = format!(
                "fn main() -> i32 {{\n  xs := [{}]\n  return xs[{idx}] as i32\n}}\n",
                elems.join(", ")
            );
            (src, wrap(vals[idx], ITy::I64))
        };
        let final_val = wrap(oracle, ITy::I32);
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffa-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {oracle}), got {code}\n--- program ---\n{src}"
        );
    }
}

// --- function-call variant: exercises the call ABI (params, args, return values) end-to-end ---

#[test]
fn function_calls_compute_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03).wrapping_add(7));
        let nfns = 1 + rng.below(3); // 1..3 helper functions
        let mut decls = String::new();
        let mut calls = String::new();
        let mut last_val = 0i128;
        let mut last_idx = 0usize;
        for i in 0..nfns {
            let ret = TYPES[rng.below(TYPES.len())];
            let arity = 1 + rng.below(3); // 1..3 parameters, each its own type (mixed → casts in body)
            let mut sig = String::new();
            let mut args = Vec::new();
            let mut pvars: Vec<(String, ITy, i128)> = Vec::new();
            for p in 0..arity {
                let pty = TYPES[rng.below(TYPES.len())];
                let av = rng.below(10) as i128; // arg literal 0..9 (representable in every width)
                if p > 0 {
                    sig.push_str(", ");
                }
                sig.push_str(&format!("p{p}: {}", pty.name()));
                args.push(av.to_string());
                pvars.push((format!("p{p}"), pty, wrap(av, pty)));
            }
            // The body's oracle is computed with the params bound to *exactly* the args main passes,
            // so each function is called once with those args and must return that value.
            let (body, result) = gen_typed(&mut rng, 3, ret, &pvars);
            decls.push_str(&format!("fn f{i}({sig}) -> {} = {body}\n", ret.name()));
            calls.push_str(&format!("  c{i}: {} := f{i}({})\n", ret.name(), args.join(", ")));
            last_val = result;
            last_idx = i;
        }
        let final_val = wrap(last_val, ITy::I32);
        let src = format!("{decls}fn main() -> i32 {{\n{calls}  return c{last_idx} as i32\n}}\n");
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("difff-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {last_val}), got {code}\n--- program ---\n{src}"
        );
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
