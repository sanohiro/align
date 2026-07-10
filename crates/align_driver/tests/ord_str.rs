//! `Ord(str)` — byte-lexicographic ordering on `str` (settled 2026-07-09). The `<`/`<=`/`>`/`>=`
//! operators compare `str` views by bytes (= Unicode scalar order for valid UTF-8), one `memcmp`
//! over the shared prefix with the shorter string ordering first on a tie; locale collation stays a
//! `pkg` concern. String keys in `sort_by_key` use the same comparator. Owned `string` ordering and
//! `bool` ordering stay rejected. These are end-to-end run tests (the sema-level accept/reject lives
//! in `align_sema`'s unit tests); each returns a small (`< 256`) exit code the harness reads back.

mod common;
use common::*;

fn run_code(name: &str, src: &str) -> i32 {
    let out = build_and_run(name, src);
    out.status.code().expect("process exited with a code")
}

#[test]
fn all_four_operators() {
    // apple < banana on every ordering operator; each true test adds 1 (expect all 7 fire).
    let src = "\
fn main() -> i32 {
  a := \"apple\"
  b := \"banana\"
  mut n := 0
  if a < b { n = n + 1 }
  if b > a { n = n + 1 }
  if a <= b { n = n + 1 }
  if b >= a { n = n + 1 }
  if a <= a { n = n + 1 }
  if a >= a { n = n + 1 }
  if a < a { n = n + 100 } else { n = n + 1 }
  return n
}
";
    assert_eq!(run_code("ord-ops", src), 7);
}

#[test]
fn equal_prefix_length_tiebreak() {
    // A proper prefix orders before the longer string: "ab" < "abc"; "abc" > "ab".
    let src = "\
fn main() -> i32 {
  mut n := 0
  if \"ab\" < \"abc\" { n = n + 1 }
  if \"abc\" > \"ab\" { n = n + 1 }
  if \"abc\" <= \"abc\" { n = n + 1 }
  if \"abcd\" < \"abd\" { n = n + 1 }
  return n
}
";
    assert_eq!(run_code("ord-prefix", src), 4);
}

#[test]
fn empty_string_is_smallest() {
    let src = "\
fn main() -> i32 {
  mut n := 0
  if \"\" < \"a\" { n = n + 1 }
  if \"a\" > \"\" { n = n + 1 }
  if \"\" <= \"\" { n = n + 1 }
  if \"\" >= \"\" { n = n + 1 }
  if \"\" < \"\" { n = n + 100 } else { n = n + 1 }
  return n
}
";
    assert_eq!(run_code("ord-empty", src), 5);
}

#[test]
fn sort_by_key_string_keys() {
    // Scalar elements sorted by a `str` key: names delta/alpha/charlie/bravo for 0/1/2/3, so the
    // by-name order is alpha(1), bravo(3), charlie(2), delta(0) -> elements [1, 3, 2, 0]. Pack the
    // sorted element order into one base-10 number and read it back.
    let src = "\
fn key(x: i64) -> str = if x == 0 { \"delta\" } else { if x == 1 { \"alpha\" } else { if x == 2 { \"charlie\" } else { \"bravo\" } } }
fn main() -> i32 {
  xs := [0, 1, 2, 3]
  sorted := xs.sort_by_key(key)
  acc := sorted[0] * 8 + sorted[1] * 4 + sorted[2] * 2 + sorted[3]
  return acc as i32
}
";
    // 1*8 + 3*4 + 2*2 + 0 = 24.
    assert_eq!(run_code("ord-sortkey", src), 24);
}

#[test]
fn sort_str_array_by_identity() {
    // A `str`-element array sorted by identity (`|s| s`): the key IS a str, exercising str-element
    // materialize + str-key compare. ["banana","apple","cherry","apple"] -> apple,apple,banana,cherry.
    let src = "\
fn id(s: str) -> str = s
fn main() -> i32 {
  xs := [\"banana\", \"apple\", \"cherry\", \"apple\"]
  sorted := xs.sort_by_key(id)
  mut n := 0
  if sorted[0] == \"apple\" { n = n + 1 }
  if sorted[1] == \"apple\" { n = n + 1 }
  if sorted[2] == \"banana\" { n = n + 1 }
  if sorted[3] == \"cherry\" { n = n + 1 }
  return n
}
";
    assert_eq!(run_code("ord-sortid", src), 4);
}

#[test]
fn bool_ordering_still_rejected() {
    // `Ord` gained `str`, not `bool` — bool ordering stays a clean compile error.
    let ok = check_errs("ord-bool", "fn main() -> i32 {\n  if true < false { return 1 }\n  return 0\n}\n");
    assert!(ok, "bool ordering must still be rejected");
}
