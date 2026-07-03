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

// --- pipeline-reducer variant: fixed array + `.map`/`.where` stages + a reduction terminal, all
// fused into one counted loop. Exercises the branchless identity-select `where` path (#303): every
// `min`/`max`/`any`/`all`/`sum`/`count`/`reduce` must equal a Rust fold over the same elements in the
// same order (wrapping i64), incl. empty selection (all filtered out) and the seed/identity endpoints.
// Elements are 0..9 (representable at every width) and every stage uses a generated named function,
// matching the call-generation style. ---

/// A generated `.map(f)` element function `f: i64 -> i64` — its `align` body and matching oracle.
#[derive(Clone, Copy)]
enum MapOp {
    AddK(i64),
    SubK(i64),
    MulK(i64),
    Square,
}
impl MapOp {
    fn pick(rng: &mut Rng) -> MapOp {
        let k = rng.below(10) as i64;
        match rng.below(4) {
            0 => MapOp::AddK(k),
            1 => MapOp::SubK(k),
            2 => MapOp::MulK(k),
            _ => MapOp::Square,
        }
    }
    fn src(self) -> String {
        match self {
            MapOp::AddK(k) => format!("x + {k}"),
            MapOp::SubK(k) => format!("x - {k}"),
            MapOp::MulK(k) => format!("x * {k}"),
            MapOp::Square => "x * x".to_string(),
        }
    }
    fn eval(self, x: i64) -> i64 {
        match self {
            MapOp::AddK(k) => x.wrapping_add(k),
            MapOp::SubK(k) => x.wrapping_sub(k),
            MapOp::MulK(k) => x.wrapping_mul(k),
            MapOp::Square => x.wrapping_mul(x),
        }
    }
}

/// A generated predicate `p: i64 -> bool` (for `.where` / `.any` / `.all`). `FilterAll` (`x > 1000`)
/// deliberately drops every 0..9 element so the empty-selection identity path is exercised.
#[derive(Clone, Copy)]
enum Pred {
    Gt(i64),
    Lt(i64),
    Ge(i64),
    Eq(i64),
    Mod(i64, i64),
    FilterAll,
}
impl Pred {
    fn pick(rng: &mut Rng) -> Pred {
        match rng.below(6) {
            0 => Pred::Gt(rng.below(10) as i64),
            1 => Pred::Lt(rng.below(10) as i64),
            2 => Pred::Ge(rng.below(10) as i64),
            3 => Pred::Eq(rng.below(10) as i64),
            4 => {
                let m = 1 + rng.below(9) as i64;
                Pred::Mod(m, rng.below(m as usize) as i64)
            }
            _ => Pred::FilterAll,
        }
    }
    fn src(self) -> String {
        match self {
            Pred::Gt(k) => format!("x > {k}"),
            Pred::Lt(k) => format!("x < {k}"),
            Pred::Ge(k) => format!("x >= {k}"),
            Pred::Eq(k) => format!("x == {k}"),
            Pred::Mod(m, r) => format!("x % {m} == {r}"),
            Pred::FilterAll => "x > 1000".to_string(),
        }
    }
    fn eval(self, x: i64) -> bool {
        match self {
            Pred::Gt(k) => x > k,
            Pred::Lt(k) => x < k,
            Pred::Ge(k) => x >= k,
            Pred::Eq(k) => x == k,
            Pred::Mod(m, r) => x.wrapping_rem(m) == r,
            Pred::FilterAll => false,
        }
    }
}

/// A generated fold function `f: (i64, i64) -> i64` for `.reduce(init, f)`.
#[derive(Clone, Copy)]
enum RedOp {
    Add,
    Mul,
    Sub,
}
impl RedOp {
    fn pick(rng: &mut Rng) -> RedOp {
        match rng.below(3) {
            0 => RedOp::Add,
            1 => RedOp::Mul,
            _ => RedOp::Sub,
        }
    }
    fn src(self) -> &'static str {
        match self {
            RedOp::Add => "a + x",
            RedOp::Mul => "a * x",
            RedOp::Sub => "a - x",
        }
    }
    fn eval(self, a: i64, x: i64) -> i64 {
        match self {
            RedOp::Add => a.wrapping_add(x),
            RedOp::Mul => a.wrapping_mul(x),
            RedOp::Sub => a.wrapping_sub(x),
        }
    }
}

#[test]
fn pipeline_reductions_compute_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0x8EBC_6AF0_9C88_C6E1).wrapping_add(17));
        // Source: an array of 3..8 elements (0..9, i64 default), then 0..2 map/where stages, then a
        // reduction terminal. The oracle folds the *same* elements through the *same* stages in Rust.
        let n = 3 + rng.below(6);
        let elems: Vec<i64> = (0..n).map(|_| rng.below(10) as i64).collect();
        let elems_src: Vec<String> = elems.iter().map(|v| v.to_string()).collect();
        let mut pipeline = format!("[{}]", elems_src.join(", "));
        let mut helpers = String::new();
        let mut hid = 0usize;
        let mut v = elems.clone();
        for _ in 0..rng.below(3) {
            if rng.below(2) == 0 {
                let op = MapOp::pick(&mut rng);
                let name = format!("mf{hid}");
                hid += 1;
                helpers.push_str(&format!("fn {name}(x: i64) -> i64 = {}\n", op.src()));
                pipeline.push_str(&format!(".map({name})"));
                v = v.iter().map(|&x| op.eval(x)).collect();
            } else {
                let p = Pred::pick(&mut rng);
                let name = format!("wf{hid}");
                hid += 1;
                helpers.push_str(&format!("fn {name}(x: i64) -> bool = {}\n", p.src()));
                pipeline.push_str(&format!(".where({name})"));
                v.retain(|&x| p.eval(x));
            }
        }
        // Terminal reduction. `min`/`max` use the branchless seed (i64::MAX / i64::MIN), so an empty
        // selection returns the seed exactly as the compiler's identity-select does.
        let (src, expected) = match rng.below(7) {
            0 => {
                let oracle = v.iter().fold(0i64, |a, &x| a.wrapping_add(x));
                (format!("fn main() -> i32 {{\n  return {pipeline}.sum() as i32\n}}\n{helpers}"), oracle as i128)
            }
            1 => {
                let oracle = v.len() as i128;
                (format!("fn main() -> i32 {{\n  return {pipeline}.count() as i32\n}}\n{helpers}"), oracle)
            }
            2 => {
                let oracle = v.iter().fold(i64::MAX, |a, &x| a.min(x));
                (format!("fn main() -> i32 {{\n  return {pipeline}.min() as i32\n}}\n{helpers}"), oracle as i128)
            }
            3 => {
                let oracle = v.iter().fold(i64::MIN, |a, &x| a.max(x));
                (format!("fn main() -> i32 {{\n  return {pipeline}.max() as i32\n}}\n{helpers}"), oracle as i128)
            }
            4 => {
                let p = Pred::pick(&mut rng);
                let name = format!("af{hid}");
                helpers.push_str(&format!("fn {name}(x: i64) -> bool = {}\n", p.src()));
                let oracle = v.iter().any(|&x| p.eval(x));
                let body = format!("fn main() -> i32 {{\n  b := {pipeline}.any({name})\n  return if b {{ 1 }} else {{ 0 }}\n}}\n{helpers}");
                (body, if oracle { 1 } else { 0 })
            }
            5 => {
                let p = Pred::pick(&mut rng);
                let name = format!("lf{hid}");
                helpers.push_str(&format!("fn {name}(x: i64) -> bool = {}\n", p.src()));
                let oracle = v.iter().all(|&x| p.eval(x));
                let body = format!("fn main() -> i32 {{\n  b := {pipeline}.all({name})\n  return if b {{ 1 }} else {{ 0 }}\n}}\n{helpers}");
                (body, if oracle { 1 } else { 0 })
            }
            _ => {
                let op = RedOp::pick(&mut rng);
                let init = rng.below(10) as i64;
                let name = format!("rf{hid}");
                helpers.push_str(&format!("fn {name}(a: i64, x: i64) -> i64 = {}\n", op.src()));
                let oracle = v.iter().fold(init, |a, &x| op.eval(a, x));
                (format!("fn main() -> i32 {{\n  return {pipeline}.reduce({init}, {name}) as i32\n}}\n{helpers}"), oracle as i128)
            }
        };
        // For the integer terminals `expected` is the i128 oracle → wrap to i32; for the bool terminals
        // it is already 0/1 (an exit code that survives the low-byte truncation unchanged).
        let final_val = wrap(expected, ITy::I32);
        let want = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffp-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, want,
            "miscompile on seed {seed}: expected {want} (oracle {expected}), got {code}\n--- program ---\n{src}"
        );
    }
}

// --- vecN elementwise-arithmetic variant: a chain of lane-wise `+ - * / %` over `vecN<T>` operands,
// observed by a constant-lane read or `.sum()`. The oracle is a lane-wise wrap fold. Every divisor
// operand is a fresh vector with lanes in 1..9, so no lane is zero (that abort semantics is covered by
// `vec_simd.rs`) and — since dividends stay small and divisors are positive — the signed `INT_MIN/-1`
// case never arises in the generated value range. ---

const VEC_TYPES: [ITy; 6] = [ITy::I16, ITy::I32, ITy::I64, ITy::U16, ITy::U32, ITy::U64];

/// Declare a fresh `vecN<T>` literal with `w` lanes each drawn from `lo..=hi`, appended to `body`.
/// Returns its name and the per-lane oracle values (normalized into `t`).
fn vec_leaf(rng: &mut Rng, body: &mut String, nid: &mut usize, t: ITy, w: usize, lo: i128, hi: i128) -> (String, Vec<i128>) {
    let lanes: Vec<i128> =
        (0..w).map(|_| wrap(lo + rng.below((hi - lo + 1) as usize) as i128, t)).collect();
    let name = format!("v{}", *nid);
    *nid += 1;
    let elems: Vec<String> = lanes.iter().map(|v| v.to_string()).collect();
    body.push_str(&format!("  {name}: vec{w}<{}> := [{}]\n", t.name(), elems.join(", ")));
    (name, lanes)
}

#[test]
fn vector_lane_arithmetic_computes_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0xC2B2_AE3D_27D4_EB4F).wrapping_add(19));
        let t = VEC_TYPES[rng.below(VEC_TYPES.len())];
        let w = [2usize, 4, 8][rng.below(3)];
        let mut body = String::new();
        let mut nid = 0usize;
        // Start with two leaf operands (lanes 0..9), then 1..3 lane-wise ops. A `/`/`%` right operand
        // is always a fresh 1..9 divisor leaf (never a computed value), guaranteeing non-zero lanes.
        let mut pool: Vec<(String, Vec<i128>)> = Vec::new();
        pool.push(vec_leaf(&mut rng, &mut body, &mut nid, t, w, 0, 9));
        pool.push(vec_leaf(&mut rng, &mut body, &mut nid, t, w, 0, 9));
        let mut last = pool[pool.len() - 1].clone();
        for _ in 0..(1 + rng.below(3)) {
            let opc = rng.below(5);
            let (ln, ll) = pool[rng.below(pool.len())].clone();
            let (rn, rl) = if opc >= 3 {
                vec_leaf(&mut rng, &mut body, &mut nid, t, w, 1, 9)
            } else {
                pool[rng.below(pool.len())].clone()
            };
            let op = ["+", "-", "*", "/", "%"][opc];
            let lanes: Vec<i128> = (0..w)
                .map(|i| {
                    let (a, b) = (ll[i], rl[i]);
                    let val = match opc {
                        0 => a + b,
                        1 => a - b,
                        2 => a * b,
                        3 => a / b, // b in 1..9 → non-zero; dividends small → no INT_MIN/-1
                        _ => a % b,
                    };
                    wrap(val, t)
                })
                .collect();
            let name = format!("r{}", nid);
            nid += 1;
            body.push_str(&format!("  {name} := {ln} {op} {rn}\n"));
            pool.push((name.clone(), lanes.clone()));
            last = (name, lanes);
        }
        // Observe either one lane (extractelement) or the horizontal `.sum()` (a lane-wise wrap fold).
        let oracle = if rng.below(2) == 0 {
            let i = rng.below(w);
            body.push_str(&format!("  return {}[{i}] as i32\n", last.0));
            last.1[i]
        } else {
            body.push_str(&format!("  return {}.sum() as i32\n", last.0));
            last.1.iter().fold(0i128, |a, &x| wrap(a + x, t))
        };
        let src = format!("fn main() -> i32 {{\n{body}}}\n");
        let final_val = wrap(oracle, ITy::I32);
        let want = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffv-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, want,
            "miscompile on seed {seed}: expected {want} (oracle {oracle}), got {code}\n--- program ---\n{src}"
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

// === wave 2: Option/Result value round-trip, enum + match, nested struct read/write ===
//
// Each generator, like the wave-1 ones, emits a **valid** Align program together with the value it
// must compute (a Rust oracle mirroring Align's semantics exactly) and asserts the process exit code
// equals it. The "small value representable at every width" trick (element/field/arg values 0..9) is
// kept so a stored value always reads back unchanged and only the final i32 sum wraps. The two
// control-flow generators additionally assert their seed range *exercises both branches* (Some/None,
// Ok/Err) so a generator that silently only ever took one path would fail loudly.

// --- Option `else`-unwrap: a chain of `pick(b) else fallback` unwraps, observed by their i32 sum.
// Each helper returns `Some(payload)` for a `true` selector and `None` for `false`; the oracle picks
// the payload or the fallback per the (statically known) selector. Both the Some and None arms are
// forced across the seed range. ---

#[test]
fn option_else_unwrap_computes_the_oracle_value() {
    if !backend_available() {
        return;
    }
    let (mut some_hit, mut none_hit) = (false, false);
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0x94D0_49BB_1331_11EB).wrapping_add(23));
        let nfns = 1 + rng.below(3); // 1..3 Option helpers
        let mut helpers = String::new();
        let mut body = String::new();
        let mut names = Vec::new();
        let mut acc = 0i128;
        for i in 0..nfns {
            // Payload of `Some` and the `else` fallback are both i32-typed expressions (they wrap at
            // i32 exactly like the binding they feed). The selector is a compile-time-known literal.
            let (payload, pv) = gen_typed(&mut rng, 2, ITy::I32, &[]);
            let (fallback, fv) = gen_typed(&mut rng, 2, ITy::I32, &[]);
            let take_some = rng.below(2) == 0;
            if take_some {
                some_hit = true;
            } else {
                none_hit = true;
            }
            helpers.push_str(&format!(
                "fn pick{i}(b: bool) -> Option<i32> {{\n  if b {{ return Some({payload}) }}\n  return None\n}}\n"
            ));
            body.push_str(&format!("  v{i} := pick{i}({take_some}) else ({fallback})\n"));
            names.push(format!("v{i}"));
            acc = wrap(acc.wrapping_add(if take_some { pv } else { fv }), ITy::I32);
        }
        let src = format!(
            "{helpers}fn main() -> i32 {{\n{body}  return ({}) as i32\n}}\n",
            names.join(" + ")
        );
        let final_val = wrap(acc, ITy::I32);
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffopt-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {acc}), got {code}\n--- program ---\n{src}"
        );
    }
    assert!(some_hit && none_hit, "seed range must exercise both the Some and None `else`-unwrap arms");
}

// --- Result `?`-propagation chain: a `run()` helper threads 2..3 `step(b)?` calls; a `true` selector
// makes `step` return `Err(error(code))`, short-circuiting the chain at the first such step; all-false
// reaches `Ok(sum)`. `main` matches the result — `Ok(v) => v`, `Err(Code(c)) => c` — so a wrong
// short-circuit point, a dropped payload, or a mis-propagated code all surface as a wrong exit code.
// Codes are 1..120 (a distinct, byte-fitting nonzero); Ok payloads are i32 expressions. ---

#[test]
fn result_question_chain_computes_the_oracle_value() {
    if !backend_available() {
        return;
    }
    let (mut ok_hit, mut err_hit) = (false, false);
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03).wrapping_add(29));
        let nsteps = 2 + rng.below(2); // 2..3 steps
        let mut helpers = String::new();
        let mut chain = String::new();
        let mut names = Vec::new();
        let mut first_err: Option<i128> = None;
        let mut sum = 0i128;
        for i in 0..nsteps {
            let (payload, pv) = gen_typed(&mut rng, 2, ITy::I32, &[]);
            let code = 1 + rng.below(120) as i128; // 1..120: nonzero, fits a byte, no exit clamp
            let take_err = rng.below(2) == 0;
            helpers.push_str(&format!(
                "fn step{i}(b: bool) -> Result<i32, Error> {{\n  if b {{ return Err(error({code})) }}\n  return Ok({payload})\n}}\n"
            ));
            chain.push_str(&format!("  x{i} := step{i}({take_err})?\n"));
            names.push(format!("x{i}"));
            // Oracle: the first `true` step's Err short-circuits `run`; only preceding Oks ran.
            if take_err && first_err.is_none() {
                first_err = Some(code);
            }
            if first_err.is_none() {
                sum = wrap(sum.wrapping_add(pv), ITy::I32);
            }
        }
        let (oracle, tag) = match first_err {
            Some(code) => {
                err_hit = true;
                (code, "err")
            }
            None => {
                ok_hit = true;
                (sum, "ok")
            }
        };
        let src = format!(
            "{helpers}fn run() -> Result<i32, Error> {{\n{chain}  return Ok({})\n}}\n\
             fn main() -> i32 = match run() {{\n  Ok(v) => v as i32,\n  Err(e) => match e {{ Code(c) => c as i32, _ => 0 }},\n}}\n",
            names.join(" + ")
        );
        let final_val = wrap(oracle, ITy::I32);
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffres-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed} ({tag} path): expected {expected} (oracle {oracle}), got {code}\n--- program ---\n{src}"
        );
    }
    assert!(ok_hit && err_hit, "seed range must exercise both the all-Ok and short-circuiting Err paths");
}

// --- enum + exhaustive match: a generated sum type (2..5 variants, mixed tag-only / 1..2 scalar
// payloads of mixed widths) with a per-variant arm that combines a distinct base constant and its
// payloads. `main` constructs 2..4 variants and sums their `eval`. Reading the payload back (0..9,
// representable at every width) and choosing a per-variant base means a mis-tagged match or a
// mis-read payload both change the sum. `eval` returns i64 so widening payload casts never truncate. ---

#[test]
fn enum_match_computes_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0xA076_1D64_78BD_642F).wrapping_add(31));
        let nv = 2 + rng.below(4); // 2..5 variants
        // Per-variant schema: arity 0..2 and a payload type per slot; a distinct base constant.
        let mut arity = Vec::new();
        let mut ptypes: Vec<Vec<ITy>> = Vec::new();
        let mut bases = Vec::new();
        for v in 0..nv {
            let a = rng.below(3); // 0..2 payload slots
            arity.push(a);
            ptypes.push((0..a).map(|_| TYPES[rng.below(TYPES.len())]).collect());
            bases.push(3 + 13 * v as i128 + rng.below(7) as i128); // distinct-ish per variant
        }
        // Declaration: `E { V0, V1(t), V2(t, t), ... }`.
        let decl_variants: Vec<String> = (0..nv)
            .map(|v| {
                if arity[v] == 0 {
                    format!("V{v}")
                } else {
                    let ts: Vec<&str> = ptypes[v].iter().map(|t| t.name()).collect();
                    format!("V{v}({})", ts.join(", "))
                }
            })
            .collect();
        // `match` arms: `Vk(a, b) => base_k + (a as i64) + (b as i64)`.
        let arms: Vec<String> = (0..nv)
            .map(|v| {
                if arity[v] == 0 {
                    format!("  V{v} => {},", bases[v])
                } else {
                    let binds: Vec<String> = (0..arity[v]).map(|p| format!("a{p}")).collect();
                    let adds: String = binds.iter().map(|b| format!(" + ({b} as i64)")).collect();
                    format!("  V{v}({}) => {}{adds},", binds.join(", "), bases[v])
                }
            })
            .collect();
        // Constructions in `main`: 2..4 randomly chosen variants with 0..9 literal args.
        let nc = 2 + rng.below(3);
        let mut calls = Vec::new();
        let mut oracle = 0i128;
        for _ in 0..nc {
            let v = rng.below(nv);
            let args: Vec<i128> = (0..arity[v]).map(|_| rng.below(10) as i128).collect();
            let ctor = if args.is_empty() {
                format!("E.V{v}")
            } else {
                let a: Vec<String> = args.iter().map(|x| x.to_string()).collect();
                format!("E.V{v}({})", a.join(", "))
            };
            calls.push(format!("eval({ctor})"));
            oracle = oracle.wrapping_add(bases[v] + args.iter().sum::<i128>());
        }
        let src = format!(
            "E {{ {} }}\nfn eval(x: E) -> i64 = match x {{\n{}\n}}\nfn main() -> i32 {{\n  return ({}) as i32\n}}\n",
            decl_variants.join(", "),
            arms.join("\n"),
            calls.join(" + ")
        );
        let final_val = wrap(oracle, ITy::I32);
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffenum-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {oracle}), got {code}\n--- program ---\n{src}"
        );
    }
}

// --- nested struct read/write chains: a depth-2..3 tower of structs, each level mixing scalar fields
// (mixed widths) with one nested-struct field placed at a random position — so the compiler's
// descending-alignment field reordering (#307) must map every logical field path to the right
// physical slot at *every* nesting level. We build with nested literals, do an optional whole-subtree
// literal write plus some leaf-path writes, then sum every leaf read back by its full path. A single
// stale logical→physical index anywhere corrupts the sum. Values are 0..9 (exact at every width). ---

#[derive(Clone)]
enum Field {
    Scalar(String, ITy),
    Nested,
}

/// Generate a literal for struct `S{k}` at value-path `prefix` (deepest recursion first), pushing
/// each scalar leaf's `(path, type, value)` so the oracle knows what every leaf holds.
fn gen_nested_instance(
    rng: &mut Rng,
    schema: &[Vec<Field>],
    k: usize,
    prefix: &str,
    leaves: &mut Vec<(String, ITy, i128)>,
) -> String {
    let mut parts = Vec::new();
    for field in &schema[k] {
        match field {
            Field::Scalar(name, ty) => {
                let v = rng.below(10) as i128; // 0..9: representable at every width
                leaves.push((format!("{prefix}.{name}"), *ty, v));
                parts.push(format!("{name}: {v}"));
            }
            Field::Nested => {
                let inner = gen_nested_instance(rng, schema, k + 1, &format!("{prefix}.n"), leaves);
                parts.push(format!("n: {inner}"));
            }
        }
    }
    format!("S{k}{{{}}}", parts.join(", "))
}

#[test]
fn nested_struct_chains_compute_the_oracle_value() {
    if !backend_available() {
        return;
    }
    for seed in 0..150u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D).wrapping_add(37));
        let levels = 2 + rng.below(2); // 2 or 3 struct levels
        // Schema: each level has 1..2 scalars (innermost 1..3) plus, unless innermost, a nested field
        // "n" inserted at a random position among the scalars (exercises reordering around it).
        let mut schema: Vec<Vec<Field>> = Vec::new();
        for k in 0..levels {
            let innermost = k == levels - 1;
            let nscalars = if innermost { 1 + rng.below(3) } else { 1 + rng.below(2) };
            let mut fields: Vec<Field> = (0..nscalars)
                .map(|s| Field::Scalar(format!("s{s}"), TYPES[rng.below(TYPES.len())]))
                .collect();
            if !innermost {
                let pos = rng.below(fields.len() + 1);
                fields.insert(pos, Field::Nested);
            }
            schema.push(fields);
        }
        // Declarations, deepest first so each struct's nested-field type is already declared.
        let mut decls = String::new();
        for k in (0..levels).rev() {
            let fs: Vec<String> = schema[k]
                .iter()
                .map(|f| match f {
                    Field::Scalar(n, t) => format!("{n}: {}", t.name()),
                    Field::Nested => format!("n: S{}", k + 1),
                })
                .collect();
            decls.push_str(&format!("S{k} {{ {} }}\n", fs.join(", ")));
        }
        // Root the tower either in a plain local (`u`, nested `StoreField`/`Field` paths) or in a
        // single-element fixed struct array (`arr[0]`, nested `StoreElemField`/`IndexField` paths) —
        // so the same reordering-heavy oracle exercises *both* the local and the element GEP seams.
        let array_rooted = rng.below(2) == 0;
        let root_path = if array_rooted { "arr[0]" } else { "u" };
        // Construct the root, recording every leaf (path, type, initial value).
        let mut leaves: Vec<(String, ITy, i128)> = Vec::new();
        let root_lit = gen_nested_instance(&mut rng, &schema, 0, root_path, &mut leaves);
        // The oracle is a path -> current-value map, updated in the exact order writes are emitted.
        use std::collections::HashMap;
        let mut vals: HashMap<String, i128> = leaves.iter().map(|(p, _, v)| (p.clone(), *v)).collect();
        let read_paths: Vec<(String, ITy)> = leaves.iter().map(|(p, t, _)| (p.clone(), *t)).collect();

        let mut body = if array_rooted {
            format!("  mut arr := [{root_lit}]\n")
        } else {
            format!("  mut u := {root_lit}\n")
        };
        // Optional whole-subtree write: rewrite `<root>.n(.n)` from a fresh literal (resets its leaves).
        if levels >= 2 && rng.below(3) == 0 {
            // Choose a nesting level k in 1..levels-1 (there is a `Nested` chain to reach it).
            let k = 1 + rng.below(levels - 1);
            let prefix = format!("{root_path}{}", ".n".repeat(k));
            let mut sub: Vec<(String, ITy, i128)> = Vec::new();
            let sub_lit = gen_nested_instance(&mut rng, &schema, k, &prefix, &mut sub);
            body.push_str(&format!("  {prefix} = {sub_lit}\n"));
            for (p, _, v) in sub {
                vals.insert(p, v); // same paths as existing leaves; just new values
            }
        }
        // Some leaf-path writes (0..=nleaves), overriding individual leaves.
        let nwrites = rng.below(read_paths.len() + 1);
        for _ in 0..nwrites {
            let (p, _) = &read_paths[rng.below(read_paths.len())];
            let nv = rng.below(10) as i128;
            body.push_str(&format!("  {p} = {nv}\n"));
            vals.insert(p.clone(), nv);
        }
        // Read every leaf back by its full path and sum (each widened to i64 — exact for 0..9).
        let reads: Vec<String> = read_paths.iter().map(|(p, _)| format!("({p} as i64)")).collect();
        body.push_str(&format!("  return ({}) as i32\n", reads.join(" + ")));
        let src = format!("{decls}fn main() -> i32 {{\n{body}}}\n");

        let oracle: i128 = read_paths.iter().fold(0i128, |a, (p, _)| a.wrapping_add(vals[p]));
        let final_val = wrap(oracle, ITy::I32);
        let expected = if cfg!(windows) { final_val as i32 } else { (final_val as i32 as u8) as i32 };
        let out = build_and_run(&format!("diffnest-{seed}"), &src);
        let code = out.status.code().unwrap_or(-1);
        assert_eq!(
            code, expected,
            "miscompile on seed {seed}: expected {expected} (oracle {oracle}), got {code}\n--- program ---\n{src}"
        );
    }
}
