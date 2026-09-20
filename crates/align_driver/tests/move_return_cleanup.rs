//! L2c owner: recursively Move returns carry one path-selected cleanup bit through every call ABI.

mod common;
use common::*;

const SOURCE: &str = r#"
fn owned(mode: i32) -> Option<string> {
  if mode == 0 { return None }
  return Some("owned".clone())
}

fn fallible(mode: i32) -> Result<string, string> {
  if mode == 0 { return Ok("ok".clone()) }
  return Err("err".clone())
}

fn invoke(handler: fn(i32) -> Result<string, string>, mode: i32) -> Result<string, string> = handler(mode)

fn relay(mode: i32) -> Result<string, string> {
  value := fallible(mode)?
  return Ok(value)
}

fn keep_error(message: string) -> string = message
fn mapped(mode: i32) -> Result<string, string> = fallible(mode).map_err(keep_error)
fn copy() -> i32 = 7

fn option_len(value: Option<string>) -> i32 = match value {
  Some(text) => text.len() as i32
  None => 0
}

fn result_len(value: Result<string, string>) -> i32 = match value {
  Ok(text) => text.len() as i32
  Err(text) => text.len() as i32
}

fn main() -> i32 = option_len(owned(1))
  + result_len(invoke(fallible, 0))
  + result_len(invoke(fallible, 1))
  + result_len(relay(0))
  + result_len(relay(1))
  + result_len(mapped(1))
  + copy()
"#;

const LOCAL_FUNCTION_VALUE_SOURCE: &str = r#"
fn fallible(ok: bool) -> Result<string, string> =
  if ok { Ok("ok".clone()) } else { Err("error".clone()) }

fn result_len(value: Result<string, string>) -> i32 = match value {
  Ok(text) => text.len() as i32
  Err(text) => text.len() as i32
}

fn main() -> i32 {
  handler := fallible
  return result_len(handler(false))
}
"#;

const TAG_ONLY_SOURCE: &str = r#"
fn make(ok: bool) -> Result<string, string> =
  if ok { Ok("yes".clone()) } else { Err("no".clone()) }

fn inspect(ok: bool) -> i32 {
  value := make(ok)
  return match value {
    Ok(text) => text.len() as i32
    Err(text) => text.len() as i32
  }
}

fn main() -> i32 = inspect(true) + inspect(false)
"#;

const USER_SUM_TAG_ONLY_SOURCE: &str = r#"
Choice { Left(string), Right(string) }

fn make_sum(left: bool) -> Choice =
  if left { Choice.Left("left".clone()) } else { Choice.Right("right".clone()) }

fn inspect_sum(left: bool) -> i32 {
  value := make_sum(left)
  return match value {
    Left(text) => text.len() as i32
    Right(text) => text.len() as i32
  }
}

fn main() -> i32 = inspect_sum(true) + inspect_sum(false)
"#;

const FRESH_STRUCT_DESTINATION_SOURCE: &str = r#"
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64

Failure { Bad }
Pair { first: string, second: string, number: i64 }
Inner { text: string }
Nested { inner: Inner, values: array<i64>, number: i64 }
Large { a: string, b: string, c: string, d: string, e: string, f: string, g: string, h: string, number: i64 }

fn number(ok: bool) -> Result<i64, Failure> {
  if ok { return Ok(9) }
  return Err(Failure.Bad)
}

fn explicit_exit() -> i32 {
  pair := Pair {
    first: "first".clone(),
    second: "second".clone(),
    number: { return 7 }
  }
  return pair.number as i32
}

fn try_exit() -> Result<i32, Failure> {
  pair := Pair {
    first: "first".clone(),
    second: "second".clone(),
    number: number(false)?
  }
  return Ok(pair.number as i32)
}

fn success() -> i32 {
  pair := Pair {
    first: "first".clone(),
    second: "second".clone(),
    number: 9
  }
  return pair.number as i32
}

fn hard_trap(divisor: i64) -> i64 {
  pair := Pair {
    first: "first".clone(),
    second: "second".clone(),
    number: 18 / divisor
  }
  return pair.number
}

fn nested_exit() -> i32 {
  nested := Nested {
    inner: Inner { text: "nested".clone() },
    values: [1, 2].to_array(),
    number: { return 11 }
  }
  return nested.number as i32
}

fn loop_exit() -> i32 {
  loop {
    pair := Pair {
      first: "first".clone(),
      second: "second".clone(),
      number: { break }
    }
  }
  return 15
}

fn loop_field() -> i32 {
  pair := Pair {
    first: "first".clone(),
    second: "second".clone(),
    number: loop { break 17 }
  }
  return pair.number as i32
}

fn returned() -> Pair {
  pair := Pair {
    first: "first".clone(),
    second: "second".clone(),
    number: 9
  }
  return pair
}

fn consume(pair: Pair) -> i32 = pair.number as i32

fn returned_large() -> Large {
  value := Large {
    a: "a".clone(), b: "b".clone(), c: "c".clone(), d: "d".clone(),
    e: "e".clone(), f: "f".clone(), g: "g".clone(), h: "h".clone(), number: 13
  }
  return value
}

fn consume_large(value: Large) -> i32 = value.number as i32

fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  if explicit_exit() != 7 { return 1 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 2 }
  _ := try_exit()
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 3 }
  if success() != 9 { return 4 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 5 }
  if hard_trap(2) != 9 { return 6 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 7 }
  if nested_exit() != 11 { return 8 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 9 }
  if loop_exit() != 15 { return 10 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 11 }
  if loop_field() != 17 { return 12 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 13 }
  if consume(returned()) != 9 { return 14 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 15 }
  if consume_large(returned_large()) != 13 { return 16 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 17 }
  return 0
}
"#;

/// One `?` per cleanup-bit provenance, inside a `DynamicBit` function (`05 §3`). The five
/// `copy_*` scope probes are the same missing-bit cell reached through each borrow-transparent
/// scope kind, which `moved_drop_flag` recurses into separately; `copy_abi` is the
/// `ReturnCleanupAbi::None` row. Every probe is called on both its `Ok` and its `Err` selector, so
/// the exit code covers the reachability axis as well as the structure.
const TRY_ERR_EDGE_SOURCE: &str = r#"
fn copy_res(flag: bool) -> Result<(), i64> {
  if flag { return Ok(()) }
  return Err(5)
}
fn move_res(flag: bool) -> Result<string, string> {
  if flag { return Ok("ok".clone()) }
  return Err("boom".clone())
}
fn mixed_res(flag: bool) -> Result<string, i64> {
  if flag { return Ok("keep".clone()) }
  return Err(2)
}

fn dyn_call(flag: bool) -> Result<string, string> {
  v := move_res(flag)?
  return Ok(v)
}
fn join(flag: bool) -> Result<string, i64> {
  v := (if flag { mixed_res(true) } else { mixed_res(false) })?
  return Ok(v)
}
fn move_local(flag: bool) -> Result<string, string> {
  r := move_res(flag)
  v := r?
  return Ok(v)
}
fn wrapper() -> Result<string, i64> {
  v: string := Ok("wrap".clone())?
  return Ok(v)
}
fn move_scope(flag: bool) -> Result<string, string> {
  v := { move_res(flag) }?
  return Ok(v)
}
fn move_task_group(flag: bool) -> Result<string, string> {
  v := task_group { move_res(flag) }?
  return Ok(v)
}
fn nested_res(flag: bool) -> Result<Result<string, i64>, i64> {
  if flag { return Ok(Ok("nest".clone())) }
  return Err(3)
}
fn nested_try(flag: bool) -> Result<string, i64> {
  v := nested_res(flag)??
  return Ok(v)
}
fn copy_call(flag: bool) -> Result<string, i64> {
  s := "call".clone()
  copy_res(flag)?
  return Ok(s)
}
fn copy_local(flag: bool) -> Result<string, i64> {
  s := "local".clone()
  r := copy_res(flag)
  r?
  return Ok(s)
}
fn copy_block(flag: bool) -> Result<string, i64> {
  s := "block".clone()
  { copy_res(flag) }?
  return Ok(s)
}
fn copy_unsafe(flag: bool) -> Result<string, i64> {
  s := "unsafe".clone()
  unsafe { copy_res(flag) }?
  return Ok(s)
}
fn copy_arena(flag: bool) -> Result<string, i64> {
  s := "arena".clone()
  arena { copy_res(flag) }?
  return Ok(s)
}
fn copy_named_arena(flag: bool) -> Result<string, i64> {
  s := "named".clone()
  arena a { copy_res(flag) }?
  return Ok(s)
}
fn copy_task_group(flag: bool) -> Result<string, i64> {
  s := "tg".clone()
  task_group { copy_res(flag) }?
  return Ok(s)
}
fn copy_abi(flag: bool) -> Result<i64, i64> {
  copy_res(flag)?
  return Ok(1)
}

fn len_or(r: Result<string, i64>) -> i64 = match r { Ok(s) => s.len() Err(e) => e }
fn text_or(r: Result<string, string>) -> i64 = match r { Ok(s) => s.len() Err(e) => e.len() }
fn code_or(r: Result<i64, i64>) -> i64 = match r { Ok(v) => v Err(e) => e }

fn main() -> i32 {
  ok := text_or(dyn_call(true)) + len_or(join(true)) + text_or(move_local(true))
    + len_or(wrapper()) + text_or(move_scope(true)) + text_or(move_task_group(true))
    + len_or(nested_try(true))
    + len_or(copy_call(true)) + len_or(copy_local(true))
    + len_or(copy_block(true)) + len_or(copy_unsafe(true)) + len_or(copy_arena(true))
    + len_or(copy_named_arena(true)) + len_or(copy_task_group(true)) + code_or(copy_abi(true))
  failed := text_or(dyn_call(false)) + len_or(join(false)) + text_or(move_local(false))
    + text_or(move_scope(false)) + text_or(move_task_group(false))
    + len_or(nested_try(false))
    + len_or(copy_call(false)) + len_or(copy_local(false))
    + len_or(copy_block(false)) + len_or(copy_unsafe(false)) + len_or(copy_arena(false))
    + len_or(copy_named_arena(false)) + len_or(copy_task_group(false)) + code_or(copy_abi(false))
  return (ok + failed) as i32
}
"#;

fn mir_text(source: &str) -> String {
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "move-return-cleanup.align", source);
    assert!(
        !checked.diags.has_errors(),
        "fixture must check:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

fn function<'a>(mir: &'a str, name: &str) -> &'a str {
    let marker = format!("fn {name}(");
    let start = mir
        .find(&marker)
        .unwrap_or_else(|| panic!("missing {marker} in MIR:\n{mir}"));
    let body = &mir[start..];
    let end = body.find("\n}").map_or(body.len(), |i| i + 2);
    &body[..end]
}

/// Whether `bit` names an SSA value this body *defines* as a runtime ownership bit: the cleanup
/// result of a `DynamicBit` call (`… -> %n`) or a load of a drop-flag slot (`%n = load _m`). This
/// is what makes the Move rows a correspondence rather than a spelling check — the forwarded bit
/// has to be the one the operand's own ownership source produced, not any SSA value in scope.
fn defines_runtime_bit(body: &str, bit: &str) -> bool {
    body.lines().map(str::trim).any(|line| {
        line.ends_with(&format!("-> {bit}")) || line.starts_with(&format!("{bit} = load _"))
    })
}

/// The cleanup bit spelled on each of a function's return edges, in block order. A `?` puts its
/// `Err` edge first: the `Ok` continuation's own return edge lives in a block created after it.
fn cleanup_bits(body: &str) -> Vec<&str> {
    body.lines()
        .filter_map(|line| line.trim().strip_prefix("return_with_cleanup "))
        .filter_map(|edge| edge.rsplit_once(", "))
        .map(|(_, bit)| bit)
        .collect()
}

/// `05 §3`: a `?` `Err` edge is a return edge, so it forwards the return ABI's cleanup bit for
/// every operand provenance — including the operand that has none, where the bit is `false`. The
/// pre-fix compiler dead-ended those seven cells in `Term::Unreachable`, so the structural half is
/// mutation-verified by the absence of `unreachable`; the execution half covers the reachability
/// axis, which no structural witness can see (an `Err` edge that is never taken hides the trap).
#[test]
fn try_err_edges_forward_a_cleanup_bit_for_every_operand_provenance() {
    let mir = mir_text(TRY_ERR_EDGE_SOURCE);
    assert!(
        !mir.contains("unreachable"),
        "no return edge may dead-end into a runtime trap:\n{mir}"
    );
    // No ownership bit: the propagated `Err` cannot own a payload its Copy-typed operand never had.
    for name in [
        "copy_call",
        "copy_local",
        "copy_block",
        "copy_unsafe",
        "copy_arena",
        "copy_named_arena",
        "copy_task_group",
    ] {
        let body = function(&mir, name);
        assert!(
            matches!(
                cleanup_bits(body).as_slice(),
                [err_edge, tail] if *err_edge == "false" && tail.starts_with('%')
            ),
            "the `?` Err edge of {name} must propagate an unowned Err beside its owned tail:\n{body}"
        );
    }
    // A runtime bit (a `DynamicBit` call, a control-flow join, a bound local's flag, a scope's or
    // a nested `?`'s recursion into one of those) is forwarded as the very SSA value its own
    // ownership source defined.
    for name in [
        "dyn_call",
        "join",
        "move_local",
        "move_scope",
        "move_task_group",
        "nested_try",
    ] {
        let body = function(&mir, name);
        let bits = cleanup_bits(body);
        assert!(
            !bits.is_empty(),
            "{name} must return through the cleanup ABI:\n{body}"
        );
        for bit in bits {
            assert!(
                defines_runtime_bit(body, bit),
                "the {name} return edge must forward a bit its own ownership source defined, \
                 got {bit}:\n{body}"
            );
        }
    }
    assert_eq!(
        cleanup_bits(function(&mir, "wrapper")).first().copied(),
        Some("true"),
        "a wrapper literal's Err edge must forward sema's static provenance"
    );
    // The sibling return edges (`tail` / `Stmt::Return`) share `terminate_return`, so they carry a
    // bit structurally here too: these three probes return only wrapper literals, whose bit is
    // sema's static provenance.
    for name in ["move_res", "mixed_res", "nested_res"] {
        let body = function(&mir, name);
        assert_eq!(
            cleanup_bits(body),
            vec!["true", "true"],
            "every tail/return edge of {name} must carry its static provenance bit:\n{body}"
        );
    }
    let copy_abi = function(&mir, "copy_abi");
    assert!(
        !copy_abi.contains("return_with_cleanup"),
        "a Copy-returning function keeps the value-only return ABI on its `?` Err edge:\n{copy_abi}"
    );
    if backend_available() {
        assert_eq!(
            build_and_run("try-err-edge-provenance", TRY_ERR_EDGE_SOURCE)
                .status
                .code(),
            Some(114),
        );
    }
}

#[test]
fn move_return_cleanup_is_explicit_in_direct_indirect_and_return_mir() {
    let mir = mir_text(SOURCE);
    assert!(
        mir.contains("call_with_cleanup program owned")
            && mir.contains("call_indirect_with_cleanup")
            && mir.contains("return_with_cleanup"),
        "Move-return value and cleanup bit must share every direct, indirect, and return edge:\n{mir}"
    );
    assert!(
        mir.contains("fn copy() -> i32 borrow=None region=None cleanup=None")
            && mir.contains("call program copy()"),
        "Copy returns must retain the value-only ABI:\n{mir}"
    );
}

#[test]
fn move_return_cleanup_executes_none_some_try_and_map_err_paths() {
    if !backend_available() {
        return;
    }
    assert_eq!(
        build_and_run("move-return-cleanup", SOURCE).status.code(),
        Some(25)
    );
}

#[test]
fn indirect_cleanup_results_use_final_value_slots_and_byte_outputs() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm(SOURCE);
    assert!(
        ir.contains("cleanup.destination") && ir.contains("call.cleanup.storage"),
        "dynamic cleanup must use its independent byte channel:\n{ir}"
    );
    assert!(
        !ir.lines()
            .any(|line| line.contains("sret({") && line.contains("i1")),
        "a cleanup bit must not remain inside indirect result storage:\n{ir}"
    );
    let main = ir
        .split("define i32 @main()")
        .nth(1)
        .and_then(|body| body.split("\n}").next())
        .expect("main body");
    assert!(
        !main.contains("call.value.storage"),
        "fresh whole locals must be the indirect value destination:\n{main}"
    );
    assert!(
        !main.contains("call.byval.storage"),
        "a final result slot must feed a target-indirect parameter directly:\n{main}"
    );
}

#[test]
fn indirect_tagged_result_reads_the_tag_before_selected_payload() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm(TAG_ONLY_SOURCE);
    let inspect = ir
        .split("define internal i32 @\"align_fn$7$696e7370656374\"")
        .nth(1)
        .and_then(|body| body.split("\n}").next())
        .expect("inspect body");
    let branch = inspect.find("br i1").expect("tag branch");
    let before_branch = &inspect[..branch];
    assert!(
        before_branch.contains("call.tag.pointer") && before_branch.contains("call.tag = load i8"),
        "the discriminator must be loaded directly from final result storage:\n{inspect}"
    );
    assert!(
        !before_branch.contains("call.value = load"),
        "the aggregate payload must not be loaded before its tag branch:\n{inspect}"
    );
    assert!(
        !inspect.contains("llvm.lifetime.end.p0(ptr %call.value.storage)"),
        "a selected-arm reload must remain inside its storage lifetime:\n{inspect}"
    );
    assert_eq!(
        build_and_run("tag-only-cleanup-result", TAG_ONLY_SOURCE)
            .status
            .code(),
        Some(5)
    );

    let sum_ir = emit_llvm(USER_SUM_TAG_ONLY_SOURCE);
    let inspect_sum = sum_ir
        .split("define internal i32 @\"align_fn$11$696e73706563745f73756d\"")
        .nth(1)
        .and_then(|body| body.split("\n}").next())
        .expect("inspect_sum body");
    let sum_branch = inspect_sum.find("br i1").expect("user-sum tag branch");
    let sum_before_branch = &inspect_sum[..sum_branch];
    assert!(
        sum_before_branch.contains("call.tag.pointer")
            && sum_before_branch.contains("call.tag = load i32"),
        "a user-sum discriminator must be loaded directly from final result storage:\n{inspect_sum}"
    );
    assert!(
        !sum_before_branch.contains("call.value = load"),
        "a user-sum payload must not be loaded before its tag branch:\n{inspect_sum}"
    );
    assert_eq!(
        build_and_run("user-sum-tag-only-cleanup-result", USER_SUM_TAG_ONLY_SOURCE)
            .status
            .code(),
        Some(9)
    );
}

#[test]
fn fresh_move_struct_destinations_cleanup_reached_partial_prefixes() {
    let mir = mir_text(FRESH_STRUCT_DESTINATION_SOURCE);
    for name in ["explicit_exit", "try_exit"] {
        let function = mir
            .split(&format!("fn {name}"))
            .nth(1)
            .and_then(|body| body.split("\n}\n").next())
            .expect("fresh destination MIR function");
        let second = function.find("drop_field _0.1").expect("second field cleanup");
        let first = function.find("drop_field _0.0").expect("first field cleanup");
        assert!(
            second < first,
            "partial fields must be dropped in reverse source order:\n{function}"
        );
        assert!(
            !function.lines().any(|line| line.starts_with("    _0 <-")),
            "the final local must not receive a whole-aggregate scratch store:\n{function}"
        );
    }
    let success = mir
        .split("fn success")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .expect("success MIR function");
    for path in ["_0.0 <-", "_0.1 <-", "_0.2 <-"] {
        assert_eq!(
            success.matches(path).count(),
            1,
            "each field must be stored exactly once into final storage:\n{success}"
        );
    }
    assert!(
        !success.lines().any(|line| line.starts_with("    _0 <-")),
        "successful construction must not publish through a whole scratch store:\n{success}"
    );
    let hard_trap = mir
        .split("fn hard_trap")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .expect("hard trap MIR function");
    assert!(
        !hard_trap.contains("drop_field"),
        "a terminal hard-error edge must not gain successor cleanup:\n{hard_trap}"
    );
    let nested = mir
        .split("fn nested_exit")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .expect("nested destination MIR function");
    let array_drop = nested
        .find("drop_field _0.1")
        .expect("nested array cleanup");
    let string_drop = nested
        .find("drop_field _0.0.0")
        .expect("nested string cleanup");
    assert!(
        array_drop < string_drop,
        "nested Move leaves must drop in reverse order:\n{nested}"
    );
    let loop_exit = mir
        .split("fn loop_exit")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .expect("loop break destination MIR function");
    let second = loop_exit
        .find("drop_field _0.1")
        .expect("loop break second field cleanup");
    let first = loop_exit
        .find("drop_field _0.0")
        .expect("loop break first field cleanup");
    assert!(second < first, "loop break cleanup must be reverse order:\n{loop_exit}");
    let loop_field = mir
        .split("fn loop_field")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .expect("loop-valued field MIR function");
    assert!(
        !loop_field.contains("drop_field"),
        "breaking a loop used as a field must preserve the enclosing partial destination:\n{loop_field}"
    );
    if backend_available() {
        let ir = emit_llvm(FRESH_STRUCT_DESTINATION_SOURCE);
        let returned = ir
            .split("define internal void @\"align_fn$14$72657475726e65645f6c61726765\"")
            .nth(1)
            .and_then(|body| body.split("\n}").next())
            .unwrap_or_else(|| panic!("returned sret definition:\n{ir}"));
        assert!(
            returned.contains("sret(")
                && !returned.contains("store %Large")
                && !returned.contains("load %Large"),
            "the caller sret destination must receive fields without a whole aggregate load/store:\n{returned}"
        );
    }
    if backend_available() {
        assert_eq!(
            build_and_run("fresh-struct-destination", FRESH_STRUCT_DESTINATION_SOURCE)
                .status
                .code(),
            Some(0),
        );
    }
}

#[test]
fn local_named_function_value_preserves_move_return_cleanup_abi() {
    let mir = mir_text(LOCAL_FUNCTION_VALUE_SOURCE);
    assert!(
        mir.contains("call_indirect_with_cleanup"),
        "a local named function value must retain the target's DynamicBit return ABI:\n{mir}"
    );
    if backend_available() {
        assert_eq!(
            build_and_run(
                "move-return-local-function-value",
                LOCAL_FUNCTION_VALUE_SOURCE
            )
            .status
            .code(),
            Some(5),
        );
    }
}

#[test]
fn imported_move_return_cleanup_matches_whole_program_and_per_unit_abi() {
    if !backend_available() {
        return;
    }
    let files = &[
        (
            "values.align",
            r#"
module values
pub fn owned(flag: bool) -> Option<string> =
  if flag { Some("cross".clone()) } else { None }
pub fn copy() -> i32 = 4
"#,
        ),
        (
            "main.align",
            r#"
import values
fn length(value: Option<string>) -> i32 = match value {
  Some(text) => text.len() as i32
  None => 0
}
fn main() -> i32 = length(values.owned(true)) + length(values.owned(false)) + values.copy()
"#,
        ),
    ];
    let whole = build_and_run_multi("move-return-import-whole", files, "main.align");
    let per_unit = build_per_unit_multi("move-return-import-per-unit", files, "main.align");
    assert_eq!(whole.status.code(), Some(9));
    assert_eq!(per_unit.link_and_run().status.code(), Some(9));
    let main_mir = &per_unit.unit("main").mir;
    assert!(
        align_mir::print::program_to_string(main_mir)
            .contains("call_with_cleanup program values$owned"),
        "the importing unit must consume the producer's DynamicBit ABI"
    );
}

#[test]
fn generic_fresh_struct_construction_matches_whole_and_per_unit_compilation() {
    if !backend_available() {
        return;
    }
    let files = &[
        (
            "values.align",
            r#"
module values
pub Wrapper<T> { value: T, label: string, number: i64 }
pub fn wrap<T>(value: T) -> Wrapper<T> {
  wrapped := Wrapper { value: value, label: "x".clone(), number: 7 }
  return wrapped
}
"#,
        ),
        (
            "main.align",
            r#"
import values
fn main() -> i32 {
  wrapped := values.wrap("a".clone())
  return (wrapped.value.len() + wrapped.label.len() + wrapped.number) as i32
}
"#,
        ),
    ];
    let whole = build_and_run_multi("fresh-struct-generic-whole", files, "main.align");
    let per_unit = build_per_unit_multi("fresh-struct-generic-per-unit", files, "main.align");
    assert_eq!(whole.status.code(), Some(9));
    assert_eq!(per_unit.link_and_run().status.code(), Some(9));
    let main_mir = align_mir::print::program_to_string(&per_unit.unit("main").mir);
    let wrap = main_mir
        .split("fn values$wrap")
        .nth(1)
        .and_then(|body| body.split("\n}\n").next())
        .unwrap_or_else(|| panic!("monomorphized generic wrapper MIR:\n{main_mir}"));
    assert!(wrap.contains(".0 <-") && wrap.contains(".1 <-") && wrap.contains(".2 <-"));
    assert!(
        !wrap.lines().any(|line| line.starts_with("    _1 <- %")),
        "the monomorphized fresh destination must not receive a whole scratch value:\n{wrap}"
    );
}
