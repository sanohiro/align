//! V006: inline primitive fields survive heap-record construction and transport.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
#[path = "helpers/indexed_array_field.rs"]
mod native;
use align_span::SourceMap;

const MODEL: &str = r#"module model
pub Tensor<T> { name: string, shape: [T; 3], empty: [i64; 0] }
pub CopyShape { shape: [i64; 3] }
pub Zero { shape: [i64; 0] }
pub NestedZero { first: Zero, second: [bool; 0] }
pub fn identity<T>(value: T) -> T = value
pub fn make(n: i64) -> Tensor<i64> = Tensor { name: "weight".clone(), shape: [n, n + 1, n + 2], empty: [] }
pub fn dimension(borrow value: Tensor<i64>, i: i64) -> i64 = value.shape[i]
pub fn zero_count(borrow value: NestedZero) -> i64 = value.first.shape.len() + value.second.len()
pub fn relay(values: array_builder<Tensor<i64>>) -> array_builder<Tensor<i64>> = values
pub fn push(borrow mut values: array_builder<Tensor<i64>>, n: i64) { values.push(make(n)) }
pub fn copy_push(borrow mut values: array_builder<CopyShape>) { values.push(CopyShape { shape: [2, 3, 4] }) }
pub fn retired_shape() -> [i64; 3] {
  mut b: array_builder<Tensor<i64>> := array_builder()
  b.push(make(10))
  values := b.build()
  shape := values[0].shape
  return shape
}
pub fn zero_builder() -> array_builder<NestedZero> = array_builder(64)
pub fn zeros(borrow mut values: array_builder<NestedZero>) { values.push(NestedZero { first: Zero { shape: [] }, second: [] }) }
"#;
const MAIN: &str = r#"module main
import model
fn main() -> i32 {
  mut builder: array_builder<model.Tensor<i64>> := array_builder(1)
  local := model.make(0)
  builder.push(local)
  mut i := 1
  loop { if i == 37 { break }; model.push(builder, i); i = i + 1 }
  forwarded := model.relay(builder)
  mut values := forwarded.build()
  dimensions := values[36].shape
  if values.len() != 37 || dimensions[2] != 38 { return 1 }
  values = { mut b: array_builder<model.Tensor<i64>> := array_builder(); b.push(model.identity(model.make(7))); b.build() }
  if dimensions[2] != 38 { return 8 }
  retired := model.retired_shape()
  if retired[2] != 12 { return 9 }
  next_dimensions := values[0].shape
  if next_dimensions[1] != 8 { return 2 }
  mut copy: array_builder<model.CopyShape> := array_builder()
  model.copy_push(copy)
  saved := copy[0]
  copied := copy.build()
  if saved.shape[2] != 4 { return 3 }
  mut zero: array_builder<model.NestedZero> := array_builder(1)
  model.zeros(zero)
  before := zero[0]
  mut frozen := zero.build()
  after := frozen[0]
  if model.zero_count(before) != 0 || model.zero_count(after) != 0 { return 4 }
  selected := frozen[..]
  empty_field := frozen[0].first.shape
  if empty_field.len() != 0 { return 10 }
  cloned := selected.to_array()
  if cloned.len() != 1 { return 11 }
  pipeline := selected.map(fn item: model.NestedZero { item.first.shape.len() }).sum()
  if pipeline != 0 { return 12 }
  value := selected[0]
  if model.zero_count(value) != 0 || selected.len() != 1 { return 5 }
  mut boxed := model.zero_builder()
  model.zeros(boxed)
  boxed_values := boxed.build()
  boxed_value := boxed_values[0]
  if boxed_values.len() != 1 || model.zero_count(boxed_value) != 0 { return 6 }
  arena out {
    mut region: array_builder<model.CopyShape> := array_builder(out)
    model.copy_push(region)
    regional := region[0]
    if regional.shape[1] != 3 { return 7 }
  }
  return 0
}
"#;

#[test]
fn lifecycle_and_modes() {
    owned_fixture::run("lifecycle_and_modes", |stage| {
        assert!(align_driver::backend_available());
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        native::execute(stage, MAIN, "lifecycle").unwrap();
    });
}

#[test]
fn imports_and_cache() {
    owned_fixture::run("imports_and_cache", |stage| {
        std::fs::write(stage.join("main.align"), MAIN).unwrap();
        let entry = stage.join("main.align");
        let context = align_driver::CacheContext::at(stage.join("cache"));
        let mut original = Vec::new();
        for round in 0..4 {
            let source = if round == 2 { MODEL.replace("empty: [i64; 0]", "empty: [bool; 0]") } else { MODEL.into() };
            std::fs::write(stage.join("model.align"), source).unwrap();
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(&mut sources, entry.to_str().unwrap(), MAIN,
                &context, align_driver::UnitReuse::Allowed);
            assert!(!built.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &built.diags));
            assert_eq!(built.units.len(), 2);
            for unit in &built.units { assert_eq!(unit.frontend.as_ref().unwrap().hit, round == 1 || round == 3, "round {round}: {}", unit.unit); }
            let programs = (0..built.units.len()).map(|i| built.materialize(i).unwrap().clone()).collect::<Vec<_>>();
            let snapshot = programs.iter().map(|program| format!("{:?}\n{}", program.structs, align_mir::print::program_to_string(program))).collect::<Vec<_>>();
            if round == 0 { original = snapshot; }
            else if round == 2 { assert!(original != snapshot, "changed fixed element graph was not materialized"); }
            else { assert!(original == snapshot, "exact cache replay changed the program"); }
            let output = native::run_programs(stage, &programs, &format!("cache-{round}"), false).unwrap();
            assert!(output.status.success(), "round {round}: {output:?}");
        }
    });
}

#[test]
fn allocation_and_partial_cleanup() {
    owned_fixture::run("allocation_and_partial_cleanup", |stage| {
        let template = r#"module main
Item { name: string, FIELDS }
Envelope { items: array<Item>, child: Item, maybe: Option<Item> }
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn make(n: i64) -> Item = Item { name: "weight".clone(), VALUES }
fn relay(b: array_builder<Item>) -> array_builder<Item> = b
fn number(fail: bool) -> Result<i64, i64> { if fail { return Err(9) }; return Ok(2) }
fn partial() -> Result<i64, i64> {
  mut b: array_builder<Item> := array_builder(1)
  b.push(make(1))
  b.push(Item { name: "partial".clone(), PARTIAL })
  return Ok(b.build().len())
}
fn exercise() -> i64 {
  mut b: array_builder<Item> := array_builder(1)
  mut n := 0
  loop { if n == 37 { break }; b.push(make(n)); n = n + 1 }
  forwarded := relay(b)
  mut values := forwarded.build()
  OBSERVE
  values = { mut replacement: array_builder<Item> := array_builder(); replacement.push(make(9)); replacement.build() }
  mut outer: array_builder<Envelope> := array_builder()
  outer.push(Envelope { items: values, child: make(4), maybe: Some(make(5)) })
  complete := outer.build()
  mut abandoned: array_builder<Item> := array_builder(1)
  abandoned.push(make(7))
  abandoned = array_builder()
  abandoned.push(make(8))
  boxed := relay(abandoned)
  return answer + complete.len()
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  start := unsafe { align_rt_alloc_count() }
  freed := unsafe { align_rt_free_count() }
  if exercise() != 39 || unsafe { align_rt_requested_live_bytes() } != 0 { return 1 }
  print(unsafe { align_rt_alloc_count() } - start)
  print(unsafe { align_rt_free_count() } - freed)
  if (partial() else -9) != -9 || unsafe { align_rt_requested_live_bytes() } != 0 { return 2 }
  return 0
}
"#;
        let fixed = template.replace("FIELDS", "shape: [i64; 3], empty: [bool; 0]")
            .replace("VALUES", "shape: [n, n + 1, n + 2], empty: []")
            .replace("PARTIAL", "shape: [1, number(true)?, 3], empty: []")
            .replace("OBSERVE", "dimensions := values[36].shape; answer := dimensions[2]");
        let expanded = template.replace("FIELDS", "first: i64, second: i64, third: i64")
            .replace("VALUES", "first: n, second: n + 1, third: n + 2")
            .replace("PARTIAL", "first: 1, second: number(true)?, third: 3")
            .replace("OBSERVE", "answer := values[36].third");
        for per_unit in [false, true] {
            let mut observed = Vec::new();
            for (label, source) in [("fixed", &fixed), ("expanded", &expanded)] {
                let programs = native::programs(stage, source, per_unit).unwrap();
                let output = native::run_programs(stage, &programs, &format!("{label}-{per_unit}"), false).unwrap();
                assert!(output.status.success(), "{label}/{per_unit}: {output:?}");
                let counts = String::from_utf8(output.stdout).unwrap().lines()
                    .map(|line| line.parse::<u64>().unwrap()).collect::<Vec<_>>();
                assert_eq!(counts.len(), 2);
                assert!(counts.iter().all(|count| *count >= 40), "inactive allocation/free observers: {counts:?}");
                observed.push(counts);
            }
            assert_eq!(observed[0], observed[1], "inline fixed fields added allocations or cleanup");
        }
    });
}

#[test]
fn rejections() {
    owned_fixture::run("rejections", |stage| {
        let mut cases = Vec::new();
        for element in ["str", "string", "Child", "raw", "fn(i64) -> i64"] {
            for length in [0, 2] {
                cases.push(format!("Child {{ x: i64 }}\nBad {{ value: [{element}; {length}] }}\nfn main() {{ b: array_builder<Bad> := array_builder() }}"));
            }
        }
        let prefix = "Item { name: string, shape: [i64; 3] }\nfn make() -> Item = Item { name: \"weight\".clone(), shape: [2, 3, 4] }\n";
        for body in [
            "fn main() { mut b: array_builder<Item> := array_builder(); value := make(); b.push(value); b.push(value) }",
            "fn main() { arena out { mut b: array_builder<Item> := array_builder(out) } }",
            "fn main() { mut b: array_builder<[i64; 3]> := array_builder() }",
            "fn main() { mut b: array_builder<Item> := array_builder(); b.push(make()); values := b.build(); print(values[0].shape[..].len()) }",
            "fn inspect(borrow value: Item) {}\nfn main() { mut b: array_builder<Item> := array_builder(); b.push(make()); values := b.build(); inspect(values[0]) }",
            "fn read(borrow value: Option<Item>) -> i64 { match value { Some(row) => row.shape[0], None => 0 } }\nfn main() {}",
            "fn escape() -> slice<i64> { value := make(); return value.shape[..] }\nfn main() {}",
            "fn main() { mut value := make(); mut b: array_builder<Item> := array_builder(); b.push(value); print(value.name) }",
            "Copy { shape: [i64; 3] }\nfn main() { mut b: array_builder<Copy> := array_builder(); values := [Copy { shape: [1, 2, 3] }]; b.append(values[..]) }",
        ] { cases.push(format!("{prefix}{body}")); }
        for (index, source) in cases.iter().enumerate() {
            let entry = stage.join(format!("reject-{index}.align"));
            std::fs::write(&entry, source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diags = if per_unit { align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), source).diags }
                    else { align_driver::check(&mut sources, entry.to_str().unwrap(), source).diags };
                let text = align_driver::format_diagnostics(&sources, &diags);
                assert!(diags.has_errors(), "accepted {index}/{per_unit}: {source}");
                assert!(!text.contains("internal error") && !text.contains("expected expression") && !text.contains("undefined name"), "invalid fixture {index}: {text}");
            }
        }
    });
}

#[test]
fn primitive_layouts() {
    owned_fixture::run("primitive_layouts", |stage| {
        let leaves = [("i8", "-7"), ("u8", "251"), ("i16", "-300"), ("u16", "60000"),
            ("i32", "-70000"), ("u32", "4000000000"), ("i64", "-9000000000"),
            ("u64", "18000000000000000000"), ("f32", "1.5"), ("f64", "-2.5"),
            ("bool", "true"), ("char", "'界'")];
        let fields = leaves.iter().enumerate().map(|(i, (ty, _))| format!("v{i}: [{ty}; 3]")).collect::<Vec<_>>().join(", ");
        let values = leaves.iter().enumerate().map(|(i, (_, value))| format!("v{i}: [{value}, {value}, {value}]")).collect::<Vec<_>>().join(", ");
        let observations = leaves.iter().enumerate().map(|(i, (_, value))| format!("v{i} := rows[36].v{i}; if v{i}[2] != {value} {{ return 1 }}")).collect::<Vec<_>>().join("\n");
        let many = (0..37).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
        let source = format!("Row {{ name: string, {fields}, many: [i64; 37] }}\nfn main() -> i32 {{\nmut b: array_builder<Row> := array_builder(1)\nmut n := 0\nloop {{ if n == 37 {{ break }}; b.push(Row {{ name: \"x\".clone(), {values}, many: [{many}] }}); n = n + 1 }}\nrows := b.build()\n{observations}\nlast := rows[36].many; if last[36] != 36 {{ return 2 }}\nreturn 0\n}}");
        native::execute(stage, &source, "primitive-layouts").unwrap();
    });
}

#[test]
fn zero_parallel_consumers() {
    owned_fixture::run("zero_parallel_consumers", |stage| {
        for length in [0, 1] {
            let source = format!(r#"Row {{ dims: [i64; {length}] }}
fn score(row: Row) -> i64 = row.dims.len() + 1
fn exercise(count: i64) -> i32 {{
  mut b: array_builder<Row> := array_builder()
  mut i := 0
  loop {{ if i == count {{ break }}; b.push(Row {{ dims: [{}] }}); i = i + 1 }}
  rows := b.build()
  mapped := rows.par_map(score).to_array()
  expected := count * ({length} + 1)
  if mapped.len() != count || mapped.sum() != expected {{ return 1 }}
  if rows.par_map(score).sum() != expected {{ return 2 }}
  kept := rows.where(fn row: Row {{ row.dims.len() == {length} }}).par_map(score).to_array()
  if kept.len() != count || kept.sum() != expected {{ return 3 }}
  rejected := rows.where(fn row: Row {{ row.dims.len() != {length} }}).par_map(score).to_array()
  if rejected.len() != 0 {{ return 4 }}
  return 0
}}
fn main() -> i32 {{
  if exercise(0) != 0 || exercise(3) != 0 || exercise(131073) != 0 {{ return 1 }}
  return 0
}}
"#, if length == 0 { "" } else { "7" });
            native::execute(stage, &source, &format!("parallel-{length}")).unwrap();
        }
    });
}

#[test]
fn zero_bounds_preserve_evaluation() {
    owned_fixture::run("zero_bounds_preserve_evaluation", |stage| {
        for built in [false, true] {
            for index in [-1, 0, 1] {
                let source = format!("Zero {{ empty: [i64; 0] }}\nfn index() -> i64 {{ print(7); return {index} }}\nfn main() {{ mut b: array_builder<Zero> := array_builder(64); b.push(Zero {{ empty: [] }}); {} unused := {}[index()] }}\n",
                    if built { "values := b.build();" } else { "" }, if built { "values" } else { "b" });
                for per_unit in [false, true] {
                    let programs = native::programs(stage, &source, per_unit).unwrap();
                    let output = native::run_programs(stage, &programs, &format!("bounds-{built}-{index}-{per_unit}"), false).unwrap();
                    assert_eq!(output.status.success(), index == 0, "{built}/{index}/{per_unit}: {output:?}");
                    assert_eq!(output.stdout, b"7\n", "index evaluation changed");
                    if index != 0 { assert!(String::from_utf8_lossy(&output.stderr).contains("index out of bounds")); }
                }
            }
        }
    });
}
