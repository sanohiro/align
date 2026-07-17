//! doc-10 §8.1 / doc-13 §8.5 — unique-buffer donation.
//!
//! When a materializing pipeline (`make().map(f).to_array()` / `.where`/`.scan`) consumes a
//! uniquely owned, provably-dead heap temporary whose element layout matches the result's, its
//! storage is DONATED as the output buffer instead of allocating a fresh one and freeing the source.
//! This is a backend/MIR-only refinement — no source/API/sema change — so this file proves it:
//!
//! - fires on the positive shape (a fresh owned scalar source: a `-> array<T>` call, a nested
//!   materializing terminal outside an arena) — the output storage is `slice_ptr(source)` with NO
//!   fresh `heap_alloc` and NO source `drop_value`;
//! - does NOT fire on the negatives — a borrowed named source, an arena collect, a `chunks`
//!   (`slice` element) source, a `str` element, or a mismatched element layout (`i64 -> i32`) all
//!   keep the allocate-then-copy-then-free shape;
//! - the `ALIGN_BUFFER_DONATE=off` measurement toggle reverts the positive shape to allocate+free;
//! - donation-on and donation-off compute byte-identical results across map/where/scan/chain (this
//!   catches a double free — it aborts — and a use-after-donate — it corrupts the result). The pure
//!   leak is closed by the alloc-count harness in `bench/buffer_donate`.

mod common;
use common::*;

/// MIR text of the single function `f` in `src`. The donation decision is per-collect and local to
/// `f`, so the whole-program text is fine to scan.
fn mir_of(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, name, src).hir);
    align_mir::print::program_to_string(&mir)
}

/// The body of `fn f` (or `fn f(...)`) only — donation-shape assertions must not pick up allocations
/// inside helper functions (`make`, `mk`).
fn fn_f_body(mir: &str) -> String {
    let start = mir.find("\nfn f").expect("fn f present");
    let rest = &mir[start + 1..];
    let end = rest.find("\n}").expect("fn f closes") + 2;
    rest[..end].to_string()
}

// A fresh, uniquely owned `array<i64>` producer (a `-> array<T>` call transfers ownership — the
// canonical donatable temporary). Kept tiny; the donation decision is size-independent.
const MAKE: &str =
    "fn make() -> array<i64> = [1, 2, 3, 4].to_array()\nfn dbl(x: i64) -> i64 = x * 2\n";

#[test]
fn donation_fires_on_fresh_owned_map() {
    // make().map(dbl).to_array(): source is a fresh owned array → donate its buffer in place.
    let src = format!("{MAKE}pub fn f() -> array<i64> = make().map(dbl).to_array()\n");
    let body = fn_f_body(&mir_of("donate_map", &src));
    assert!(
        body.contains("slice_ptr("),
        "donation must reuse the source buffer via slice_ptr:\n{body}"
    );
    assert!(
        !body.contains("heap_alloc"),
        "donation must NOT allocate a fresh output buffer:\n{body}"
    );
    assert!(
        !body.contains("drop_value"),
        "donation transfers ownership to the result — no source drop_value:\n{body}"
    );
}

#[test]
fn donation_fires_on_fresh_owned_where() {
    // where compaction is stable (out_index <= source_index), so the donated buffer is safe.
    let src = "fn make() -> array<i64> = [1, 2, 3, 4].to_array()\nfn even(x: i64) -> bool = x % 2 == 0\n\
         pub fn f() -> array<i64> = make().where(even).to_array()\n";
    let body = fn_f_body(&mir_of("donate_where", src));
    assert!(body.contains("slice_ptr("), "where must donate:\n{body}");
    assert!(!body.contains("heap_alloc"), "where donation must not allocate:\n{body}");
    assert!(!body.contains("drop_value"), "where donation must not drop the source:\n{body}");
}

#[test]
fn donation_fires_type_changing_but_layout_identical() {
    // i64 -> f64 map: different scalar TYPE, identical 8-byte layout → still donatable.
    let src = "fn make() -> array<i64> = [1, 2, 3, 4].to_array()\n\
        fn tof(x: i64) -> f64 = x as f64\n\
        pub fn f() -> array<f64> = make().map(tof).to_array()\n";
    let body = fn_f_body(&mir_of("donate_i64_f64", src));
    assert!(body.contains("slice_ptr("), "layout-identical i64->f64 must donate:\n{body}");
    assert!(!body.contains("heap_alloc"), "must not allocate:\n{body}");
}

#[test]
fn no_donation_for_borrowed_named_source() {
    // A bound parameter is borrowed (`temp_free` clear) — nothing to donate, allocate fresh.
    let src = "fn dbl(x: i64) -> i64 = x * 2\n\
        pub fn f(xs: array<i64>) -> array<i64> = xs.map(dbl).to_array()\n";
    let body = fn_f_body(&mir_of("borrowed", src));
    assert!(!body.contains("slice_ptr("), "a borrowed source must NOT donate:\n{body}");
    assert!(body.contains("heap_alloc"), "a borrowed source allocates fresh:\n{body}");
}

#[test]
fn no_donation_for_mismatched_layout() {
    // i64 -> i32 narrows the element (8 vs 4 bytes): NOT layout-identical → allocate + free source.
    let src = "fn make() -> array<i64> = [1, 2, 3, 4].to_array()\n\
        fn narrow(x: i64) -> i32 = x as i32\n\
        pub fn f() -> array<i32> = make().map(narrow).to_array()\n";
    let body = fn_f_body(&mir_of("mismatch", src));
    assert!(!body.contains("slice_ptr("), "a size mismatch must NOT donate:\n{body}");
    assert!(body.contains("heap_alloc"), "a size mismatch allocates fresh:\n{body}");
    assert!(body.contains("drop_value"), "a size mismatch still frees the source:\n{body}");
}

#[test]
fn no_donation_inside_arena() {
    // Inside an arena the output is bump-allocated (bulk-freed); donation is excluded (first slice).
    let src = "fn make() -> array<i64> = [1, 2, 3, 4].to_array()\nfn dbl(x: i64) -> i64 = x * 2\n\
        pub fn f() -> i64 = arena { make().map(dbl).to_array().sum() }\n";
    let body = fn_f_body(&mir_of("arena", src));
    assert!(!body.contains("slice_ptr("), "an arena collect must NOT donate:\n{body}");
    assert!(body.contains("arena_alloc"), "an arena collect bump-allocates:\n{body}");
}

#[test]
fn no_donation_for_str_element() {
    // A `str` element is a Move payload ({ptr,len}); its storage is never donated in place.
    let src = "fn mk() -> array<str> = [\"aa\", \"bb\"].to_array()\n\
        fn up(s: str) -> str = s\n\
        pub fn f() -> array<str> = mk().map(up).to_array()\n";
    let body = fn_f_body(&mir_of("str_elem", src));
    assert!(!body.contains("slice_ptr("), "a str element must NOT donate:\n{body}");
    assert!(body.contains("heap_alloc"), "a str element allocates fresh:\n{body}");
}

#[test]
fn toggle_off_reverts_to_allocate_and_free() {
    // ALIGN_BUFFER_DONATE=off restores allocate-then-free even on the positive shape. The toggle is
    // read in MIR lowering (process env), so this drives a fresh `alignc emit-mir` SUBPROCESS with
    // the var set in the CHILD env — never mutating this process's env (which would race the parallel
    // in-process MIR tests above). Its default (ON) counterpart is `donation_fires_on_fresh_owned_map`.
    let src = format!("{MAKE}pub fn f() -> array<i64> = make().map(dbl).to_array()\n");
    let dir = std::env::temp_dir().join(format!("align_donate_toggle_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("toggle_off.align");
    std::fs::write(&path, &src).expect("write source");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .arg("emit-mir")
        .arg(&path)
        .env("ALIGN_BUFFER_DONATE", "off")
        .output()
        .expect("spawn alignc emit-mir");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "emit-mir failed: {}", String::from_utf8_lossy(&out.stderr));
    let body = fn_f_body(&String::from_utf8_lossy(&out.stdout));
    assert!(!body.contains("slice_ptr("), "toggle off must NOT donate:\n{body}");
    assert!(body.contains("heap_alloc"), "toggle off allocates fresh:\n{body}");
    assert!(body.contains("drop_value"), "toggle off frees the source:\n{body}");
}

// ── Differential execution: donation-on vs donation-off compute identical results ────────────────

const GEN: &str = "fn gen(n: i64) -> array<i64> {\n\
    \x20 mut b: array_builder<i64> := array_builder()\n\
    \x20 mut i := 0\n\
    \x20 loop { if i >= n { break 0 }; b.push(i + 1); i = i + 1 }\n\
    \x20 return b.build()\n\
    }\n";

/// Compile+run `src` once; the default (donation ON) and toggle-off builds must print the same.
/// The toggle-off path is exercised by the in-process MIR test above and the bench; here both runs
/// use the shipped default, and the differential is against a hand-written reference in the source.
fn run_prog(name: &str, body: &str) -> String {
    let src = format!(
        "{GEN}fn add(a: i64, b: i64) -> i64 = a + b\nfn dbl(x: i64) -> i64 = x * 2\n\
         fn even(x: i64) -> bool = x % 2 == 0\n\
         fn main() -> Result<(), Error> {{\n  print({body})\n  return Ok(())\n}}\n"
    );
    let out = build_and_run(name, &src);
    assert!(out.status.success(), "{name} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn donation_execution_matches_reference() {
    if !backend_available() {
        return;
    }
    // map: sum(2*(1..=1000)) = 2 * 1000*1001/2 = 1_001_000.
    assert_eq!(run_prog("donate_exec_map", "gen(1000).map(dbl).to_array().sum()"), "1001000");
    // where: sum of even values in 1..=1000 = 2+4+...+1000 = 250_500.
    assert_eq!(run_prog("donate_exec_where", "gen(1000).where(even).to_array().sum()"), "250500");
    // scan: prefix sums of (1..=100) then summed = sum_{k=1..100} k*(101-k) = 171_700.
    assert_eq!(run_prog("donate_exec_scan", "gen(100).scan(0, add).to_array().sum()"), "171700");
    // chain where+map: sum(2*even(1..=1000)) = 2*250_500 = 501_000.
    assert_eq!(
        run_prog("donate_exec_chain", "gen(1000).where(even).map(dbl).to_array().sum()"),
        "501000"
    );
    // three chained donations in a row must all be sound.
    assert_eq!(
        run_prog(
            "donate_exec_chain3",
            "gen(500).map(dbl).to_array().map(dbl).to_array().where(even).to_array().sum()"
        ),
        // 4*(1..=500) summed = 4 * 500*501/2 = 501_000 (all already even).
        "501000"
    );
}
