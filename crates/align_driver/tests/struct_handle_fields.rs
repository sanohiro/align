//! Move **handle** struct fields (F1② of the pkg.web plan — the request `Ctx` owning its
//! `http_request_ctx`). A struct field may be a bare pointer handle (a `buffer`, `file`,
//! reader/writer, socket, http request/response/…/ctx/stream, cli command/parsed). Such a field
//! makes the enclosing struct a **Move** type whose recursive drop closes/frees the handle exactly
//! once (`drop_struct_fields`'s handle arm → the null-safe `*_free`, shared with a standalone
//! handle local via `handle_free_fn`). Move-once discipline is enforced by `MoveCheck`.

mod common;
use common::*;

#[test]
fn handle_field_construct_and_drop() {
    if !backend_available() {
        return;
    }
    // A struct owning a `buffer` handle: build it, read a scalar field, then let it drop at scope
    // exit. A clean exit (0) proves `drop_struct_fields` freed the handle exactly once (its
    // `buffer_free` is null-safe; a leak/double-free would abort).
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := Holder { buf: buffer(64), tag: 7 }\n",
        "  print(h.tag)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn handle_field_struct_moved_into_fn_no_double_free() {
    if !backend_available() {
        return;
    }
    // The Move struct is consumed by a by-value call: the callee drops the handle, and the caller's
    // drop flag must suppress a second free (a double `buffer_free` would abort). Clean exit proves
    // the handle is freed exactly once across the move.
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn consume(h: Holder) -> i64 = h.tag\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := Holder { buf: buffer(64), tag: 7 }\n",
        "  print(consume(h))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-move", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn handle_field_owned_struct_returned_then_dropped() {
    if !backend_available() {
        return;
    }
    // A struct that *owns* its handle borrows nothing → its region is Static, so it is freely
    // returnable (the owner moves with it). The caller then drops it once.
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn make(n: i64) -> Holder = Holder { buf: buffer(64), tag: n }\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := make(42)\n",
        "  print(h.tag)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-return", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn handle_field_use_after_move_rejected() {
    // Consuming the Move struct twice is a use-after-move — a clean error, not a double-free.
    assert!(check_errs(
        "handlefield-uam",
        "Holder { buf: buffer, tag: i64 }\nfn consume(h: Holder) -> i64 = h.tag\nfn main() -> Result<(), Error> {\n  h := Holder { buf: buffer(64), tag: 7 }\n  print(consume(h))\n  print(consume(h))\n  return Ok(())\n}\n"
    ));
}

#[test]
fn handle_field_partial_move_out_nulls_the_field() {
    if !backend_available() {
        return;
    }
    // Moving a Move-handle field OUT of a struct (`b := h.buf`) is supported: the new binding takes
    // the handle and the struct's field is NULLED, so the struct's drop frees null there instead of
    // double-freeing. Only that field is consumed — the struct's other fields stay readable. This is
    // exactly what lets pkg.web's `Ctx` hand its owned `http_request_ctx` to a consuming responder
    // (`c.req.respond(rb)`); a clean exit proves the handle is freed once.
    let src = concat!(
        "Holder { buf: buffer, tag: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  h := Holder { buf: buffer(64), tag: 7 }\n",
        "  b := h.buf\n",
        "  print(h.tag)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("handlefield-partial", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn handle_field_reuse_after_partial_move_rejected() {
    // The field itself is consumed by the move: touching it again is a use-after-move, so the
    // nulling can never be observed as a live handle.
    assert!(check_errs(
        "handlefield-partial-reuse",
        "Holder { buf: buffer, tag: i64 }\nfn use2(x: buffer) -> i64 = 1\nfn main() -> Result<(), Error> {\n  h := Holder { buf: buffer(64), tag: 7 }\n  b := h.buf\n  c := h.buf\n  return Ok(())\n}\n"
    ));
}

#[test]
fn http_request_ctx_field_type_checks() {
    // The pkg.web target shape: the request `Ctx` **owns** its `http_request_ctx` handle (now a
    // nameable surface type) alongside a `str` view field. It type-checks (running needs a live
    // server — exercised by the W2 integration tests later).
    assert!(!check_errs(
        "ctx-field",
        "Ctx { req: http_request_ctx, path: str }\nfn take(c: Ctx) -> str = c.path\nfn main() -> Result<(), Error> { return Ok(()) }\n"
    ));
}

#[test]
fn option_of_handle_field_type_checks() {
    // Tagged Move payloads use the recursive field/drop path, so an optional request context is a
    // valid owning field. Runtime construction and destruction of tagged handles is covered by the
    // tagged Move payload owner tests; this test pins the handle-specific field-formation surface.
    assert!(!check_errs(
        "opt-handle",
        "H { x: Option<http_request_ctx> }\nfn main() -> Result<(), Error> { return Ok(()) }\n"
    ));
}

#[test]
fn all_field_kinds_coexist_and_drop_cleanly() {
    if !backend_available() {
        return;
    }
    // All four F1 field kinds in one Move struct: a handle (`buffer`), an owned `string`, a
    // `slice<str>` view, a fn value, and a scalar. Call the fn field, read a view element, then let
    // the struct drop — the handle + string are freed once each, the slice/fn/scalar skipped.
    let src = concat!(
        "fn hd(n: i64) -> i64 = n + 1\n",
        "Mix { req: buffer, title: string, tags: slice<str>, handler: fn(i64) -> i64, n: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  ts := [\"a\", \"b\"]\n",
        "  m := Mix { req: buffer(32), title: \"t\".clone(), tags: ts, handler: hd, n: 5 }\n",
        "  print(m.handler(m.n))\n",
        "  print(m.tags[0])\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("mix-fields", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\na\n");
}

fn handle_project(tag: &str, source: &str) -> Proj {
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("align-handle-{}-{tag}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).expect("exclusively create fixture directory");
    let project = Proj {
        dir,
        entry: "main.align".to_owned(),
    };
    project.write("main.align", source);
    project
}

fn handle_command(
    command: &mut std::process::Command,
    project: &Proj,
    label: &str,
) -> std::process::Output {
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};
    struct ChildGuard(Option<std::process::Child>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                if let Ok(pid) = i32::try_from(child.id()) {
                    // The child owns a fresh process group; an unwinding owner reaps its helpers.
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let stdout = project.dir.join(format!("{label}.stdout"));
    let stderr = project.dir.join(format!("{label}.stderr"));
    command
        .process_group(0)
        .stdout(std::fs::File::create(&stdout).expect("stdout log"))
        .stderr(std::fs::File::create(&stderr).expect("stderr log"));
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut child =
        ChildGuard(Some(command.spawn().unwrap_or_else(|error| {
            panic!("{label}: spawn fixture command: {error}")
        })));
    let status = loop {
        if let Some(status) = child
            .0
            .as_mut()
            .expect("armed child")
            .try_wait()
            .expect("poll child")
        {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "{label} exceeded its execution budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    child.0.take();
    std::process::Output {
        status,
        stdout: std::fs::read(stdout).expect("stdout"),
        stderr: std::fs::read(stderr).expect("stderr"),
    }
}

#[test]
fn borrowed_handle_receivers_preserve_nested_and_optional_owners() {
    let helper = r#"module handles
pub Stream { data: buffer, sink: Option<writer> }
pub Outer { stream: Stream }
pub Direct { sink: writer }
pub fn view(borrow owner: Outer) -> slice<u8> = owner.stream.data.bytes()
pub fn emit(borrow owner: Outer) -> Result<(), Error> {
  match owner.stream.sink {
    Some(sink) => { sink.write(owner.stream.data.bytes())?; sink.flush()? },
    None => {},
  }
  return Ok(())
}
pub fn encode(borrow mut owner: Outer) {
  mut bytes := owner.stream.data.bytes()
  bytes[0] = 66
}
pub fn direct(borrow owner: Direct) -> Result<(), Error> { owner.sink.write("!")?; owner.sink.flush()?; return Ok(()) }
pub fn optional(borrow sink: Option<writer>) -> Result<(), Error> {
  match sink { Some(active) => { active.write("?")? }, None => {} }
  return Ok(())
}
"#;
    let source = r#"module main
import handles
import std.io
fn make() -> handles.Outer {
  mut data := buffer(8)
  data.put_u8(65)
  return handles.Outer { stream: handles.Stream { data: data, sink: Some(io.stdout) } }
}
fn main() -> Result<(), Error> {
  mut owner := make()
  handles.emit(owner)?
  handles.emit(owner)?
  print(handles.view(owner).u8(0))
  handles.encode(owner)
  handles.emit(owner)?
  handles.optional(owner.stream.sink)?
  sink := handles.Direct { sink: io.stdout }
  handles.direct(sink)?
  return Ok(())
}
"#;
    let files = [("main.align", source), ("handles.align", helper)];
    let checked = diff_check_multi("borrowed-handle-receivers", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for out in [
            build_and_run_multi("borrowed-handle-receivers-whole", &files, "main.align"),
            build_per_unit_multi("borrowed-handle-receivers-unit", &files, "main.align")
                .link_and_run(),
        ] {
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, b"AA65\nB?!");
        }
    }
}

#[test]
fn borrowed_handle_projections_reject_consumption_and_escape() {
    let prelude = r#"
Holder { sink: Option<writer>, data: buffer }
fn take(sink: writer) {}
fn change(borrow mut holder: Holder) { holder = Holder { sink: None, data: buffer(1) } }
"#;
    for (name, body) in [
        (
            "return",
            "fn bad(borrow holder: Holder) -> writer { match holder.sink { Some(sink) => { return sink }, None => {} }; loop {} }",
        ),
        (
            "consume",
            "fn bad(borrow holder: Holder) { match holder.sink { Some(sink) => { take(sink) }, None => {} } }",
        ),
        (
            "join",
            "fn bad(borrow holder: Holder) { match holder.sink { Some(sink) => { value := if true { sink } else { sink }; take(value) }, None => {} } }",
        ),
        (
            "store",
            "fn bad(borrow holder: Holder) { match holder.sink { Some(sink) => { value := Holder { sink: Some(sink), data: buffer(1) } }, None => {} } }",
        ),
        (
            "capture",
            "fn bad(borrow holder: Holder) { match holder.sink { Some(sink) => { callback := fn() { result := sink.flush() } }, None => {} } }",
        ),
        (
            "alias",
            "fn bad(borrow holder: Holder) { data := holder.data }",
        ),
        (
            "replace",
            "fn bad(borrow mut holder: Holder) { view := holder.data.bytes(); change(holder); print(view.u8(0)) }",
        ),
        (
            "arg_replace",
            "fn bad(borrow mut holder: Holder) -> Result<(), Error> { match holder.sink { Some(sink) => { sink.write({ change(holder); \"bad\" })? }, None => {} }; return Ok(()) }",
        ),
        (
            "local_return",
            "fn bad() -> slice<u8> { holder := Holder { sink: None, data: buffer(1) }; return holder.data.bytes() }",
        ),
        (
            "exclusive_field",
            "fn fill(borrow mut data: buffer) { data.put_u8(0) }\nfn bad(borrow mut holder: Holder) { fill(holder.data) }",
        ),
    ] {
        let source = format!("{prelude}\n{body}\nfn main() {{}}\n");
        let checked = diff_check_multi(
            &format!("borrowed-handle-{name}"),
            &[("main.align", source.as_str())],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name} must reject on both paths; whole:\n{}\nper-unit:\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
    }
}

#[test]
fn borrowed_buffer_views_follow_optional_array_and_generic_sources() {
    let helper = r#"module views
pub Item<T> { data: T }
pub Rows { items: Option<array<Item<buffer>>> }
pub View { data: slice<u8> }
pub fn bytes(borrow item: Item<buffer>) -> slice<u8> = item.data.bytes()
pub fn text(borrow data: buffer) -> str = data.bytes().as_str() else ""
pub fn optional(borrow data: Option<buffer>) -> slice<u8> = match data {
  Some(active) => active.bytes(), None => { empty: slice<u8> := []; empty },
}
pub Data { Present(Item<buffer>), Absent }
pub fn tagged(borrow data: Data) -> slice<u8> = match data {
  Present(active) => active.data.bytes(), Absent => { empty: slice<u8> := []; empty },
}
pub fn result(borrow data: Result<buffer, i32>) -> slice<u8> = match data {
  Ok(active) => active.bytes(), Err(_) => { empty: slice<u8> := []; empty },
}
pub fn first(borrow rows: Rows) -> slice<u8> = match rows.items {
  Some(items) => bytes(items[0]), None => { empty: slice<u8> := []; empty },
}
pub fn retain(borrow item: Item<buffer>, borrow mut result: View) {
  result = View { data: bytes(item) }
}
pub fn identity<T>(borrow item: Item<T>) -> i64 = 1
"#;
    for (name, body) in [
        (
            "returned",
            "view := views.bytes(item); item = views.Item { data: buffer(8) }; print(view.u8(0))",
        ),
        (
            "retained",
            "mut kept := views.View { data: [] }; views.retain(item, kept); item = views.Item { data: buffer(8) }; print(kept.data.u8(0))",
        ),
        (
            "indirect",
            "reader := views.text; view := reader(item.data); item = views.Item { data: buffer(8) }; print(view)",
        ),
        (
            "optional",
            "mut optional: Option<buffer> := Some(buffer(8)); view := views.optional(optional); optional = None; print(view.u8(0))",
        ),
        (
            "result",
            "mut result: Result<buffer, i32> := Ok(buffer(8)); view := views.result(result); result = Err(1); print(view.u8(0))",
        ),
        (
            "tagged",
            "mut tagged: views.Data := views.Present(views.Item { data: buffer(8) }); view := views.tagged(tagged); tagged = views.Absent; print(view.u8(0))",
        ),
    ] {
        let invalid = format!(
            "module main\nimport views\nfn main() {{ mut item := views.Item {{ data: buffer(8) }}; {body} }}\n"
        );
        let checked = diff_check_multi(
            &format!("borrowed-buffer-invalidated-{name}"),
            &[("main.align", invalid.as_str()), ("views.align", helper)],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name} must reject after source replacement; whole:\n{}\nper-unit:\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
        assert!(
            checked.whole_diags.contains("use of invalidated borrow") && checked.per_unit_diags.contains("use of invalidated borrow"),
            "{name} must reject for borrowing; whole:\n{}\nper-unit:\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
    }
    let source = r#"module main
import views
fn main() -> i32 {
  mut data := buffer(8)
  data.put_u8(65)
  optional: Option<buffer> := Some(data)
  if views.optional(optional).u8(0) != 65 { return 1 }
  mut other := buffer(8)
  other.put_u8(66)
  item := views.Item { data: other }
  if views.identity(item) != 1 { return 2 }
  reader := views.text
  if reader(item.data) != "B" { return 6 }
  mut kept := views.View { data: [] }
  views.retain(item, kept)
  if kept.data.u8(0) != 66 { return 3 }
  rows := views.Rows { items: None }
  if views.first(rows).len() != 0 { return 4 }
  return 0
}
"#;
    let files = [("main.align", source), ("views.align", helper)];
    let checked = diff_check_multi("borrowed-buffer-provenance", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for out in [
            build_and_run_multi("borrowed-buffer-provenance-whole", &files, "main.align"),
            build_per_unit_multi("borrowed-buffer-provenance-unit", &files, "main.align")
                .link_and_run(),
        ] {
            assert_eq!(
                out.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn borrowed_handle_io_failure_retires_the_containing_owner() {
    if !backend_available() || !cc_available() {
        return;
    }
    let Some(llc) = align_driver::llvm_tool("llc") else {
        return;
    };
    let source = r#"
import std.fs
extern "C" {
  fn probe_reset()
  fn probe_counts() -> i32
  fn probe_next_fd() -> i32
  fn probe_open(fd: i32) -> i32
  fn probe_readonly(fd: i32) -> i32
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
}
Stream { data: buffer, sink: Option<writer> }
fn emit(borrow stream: Stream) -> Result<(), Error> {
  match stream.sink {
    Some(sink) => { sink.write(stream.data.bytes()).map_err(fn error: Error { error })?; sink.flush()? },
    None => {},
  }
  return Ok(())
}
fn skip(borrow stream: Stream) -> Result<(), Error> {
  match stream.sink {
    Some(sink) => { sink.write({ return Ok(()); "unreached" })? },
    None => {},
  }
  return Ok(())
}
fn exercise(fd: i32, fail: bool) -> Result<(), Error> {
  mut data := buffer(8)
  data.put_u8(65)
  sink := fs.create("/dev/null")?
  stream := Stream { data: data, sink: Some(sink) }
  if unsafe { probe_open(fd) } != 1 { print("missing descriptor") }
  skip(stream)?
  mut iteration := 0
  loop { if iteration == 2 { break }; emit(stream)?; iteration = iteration + 1 }
  emit(stream) else ()
  if fail { if unsafe { probe_readonly(fd) } != 0 { print("failure setup") } }
  emit(stream)?
  if unsafe { probe_open(fd) } != 1 { print("premature close") }
  return Ok(())
}
fn once(fail: bool) -> i32 {
  unsafe { probe_reset() }
  fd := unsafe { probe_next_fd() }
  if fd < 0 { return 1 }
  result := exercise(fd, fail)
  match result { Ok(_) => { if fail { return 2 } }, Err(_) => { if !fail { return 3 } } }
  if unsafe { probe_open(fd) } != 0 { return 4 }
  if unsafe { probe_counts() } != 11 { return 6 }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 5 }
  return 0
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  a := once(false)
  if a != 0 { return a }
  return once(true)
}
"#;
    let native = r#"
#include <fcntl.h>
#include <stdint.h>
#include <unistd.h>
extern void align_rt_buffer_free(void *);
extern void align_rt_io_writer_free(void *);
extern int32_t align_rt_io_writer_write(void *, const uint8_t *, int64_t);
static void *last_buffer, *last_writer;
static int buffer_frees, writer_frees, writes, bad_bytes;
void probe_reset(void) { last_buffer = last_writer = 0; buffer_frees = writer_frees = writes = bad_bytes = 0; }
int32_t probe_counts(void) { return buffer_frees + 10 * writer_frees + 100 * (writes != 4 || bad_bytes); }
int32_t probe_writer_write(void *p, const uint8_t *data, int64_t len) {
    ++writes;
    if (len != 1 || !data || data[0] != 65) bad_bytes = 1;
    return align_rt_io_writer_write(p, data, len);
}
void probe_buffer_free(void *p) {
    if (p) { ++buffer_frees; if (p == last_buffer) return; last_buffer = p; }
    align_rt_buffer_free(p);
}
void probe_writer_free(void *p) {
    if (p) { ++writer_frees; if (p == last_writer) return; last_writer = p; }
    align_rt_io_writer_free(p);
}
int32_t probe_next_fd(void) {
    int fd = dup(STDOUT_FILENO);
    if (fd >= 0) close(fd);
    return fd;
}
int32_t probe_open(int32_t fd) { return fcntl(fd, F_GETFD) >= 0; }
int32_t probe_readonly(int32_t fd) {
    int input = open("/dev/null", O_RDONLY);
    if (input < 0) return -1;
    int result = dup2(input, fd);
    close(input);
    return result == fd ? 0 : -1;
}
"#;
    // Redirect only the generated program's free calls through counting delegates. The actual
    // runtime implementations, handle layouts, I/O and error paths remain the linked production
    // runtime. Counting non-null calls detects duplicate frees without invoking allocator UB.
    for per_unit in [false, true] {
        let project = handle_project("counted", source);
        let entry = project.dir.join("main.align");
        let name = entry.to_str().expect("UTF-8 fixture path");
        let mut sm = SourceMap::new();
        let programs = if per_unit {
            let walked = build_per_unit(&mut sm, name, source);
            assert!(
                !walked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &walked.diags)
            );
            walked
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>()
        } else {
            let checked = check(&mut sm, name, source);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            vec![lower_to_mir(&checked.hir)]
        };
        let mut objects = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let ir = emit_llvm_ir(program, BuildTarget::Baseline, false, &[], false).expect("LLVM");
            let ir = ir
                .replace("@align_rt_buffer_free(", "@probe_buffer_free(")
                .replace("@align_rt_io_writer_free(", "@probe_writer_free(")
                .replace("@align_rt_io_writer_write(", "@probe_writer_write(");
            let input = project.dir.join(format!("unit{index}.ll"));
            let object = project.dir.join(format!("unit{index}.o"));
            std::fs::write(&input, ir).expect("write counted LLVM");
            let compiled = handle_command(
                std::process::Command::new(&llc)
                    .args(["-filetype=obj", "-relocation-model=pic"])
                    .arg(&input)
                    .arg("-o")
                    .arg(&object),
                &project,
                &format!("llc{index}"),
            );
            assert!(
                compiled.status.success(),
                "{}",
                String::from_utf8_lossy(&compiled.stderr)
            );
            objects.push(object);
        }
        let c_source = project.dir.join("probe.c");
        let c_object = project.dir.join("probe.o");
        std::fs::write(&c_source, native).expect("write native probe");
        let compiled = handle_command(
            std::process::Command::new("cc")
                .args(["-std=c11", "-c"])
                .arg(&c_source)
                .arg("-o")
                .arg(&c_object),
            &project,
            "cc",
        );
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        objects.push(c_object);
        let refs = objects
            .iter()
            .map(|object| object.as_path())
            .collect::<Vec<_>>();
        let exe = project.dir.join("probe");
        link_objects(
            &align_driver::CDriver::default(),
            &refs,
            &exe,
            &[],
            Profile::Release,
        )
        .expect("link");
        let out = handle_command(&mut std::process::Command::new(exe), &project, "run");
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stdout.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

#[test]
fn borrowed_handle_receiver_cache_replays_and_invalidates_body_edits() {
    if !backend_available() {
        return;
    }
    let source = "import std.io\nHolder { sink: Option<writer> }\nfn emit(borrow holder: Holder) -> Result<(), Error> { match holder.sink { Some(sink) => { sink.write(\"A\")? }, None => {} }; return Ok(()) }\nfn main() -> Result<(), Error> { holder := Holder { sink: Some(io.stdout) }; emit(holder)?; return Ok(()) }\n";
    let project = handle_project("cache", source);
    for (index, text) in [
        source.to_owned(),
        source.to_owned(),
        source.replace("\"A\"", "\"B\""),
        source.to_owned(),
    ]
    .iter()
    .enumerate()
    {
        project.write("main.align", text);
        let output = handle_command(
            std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
                .current_dir(&project.dir)
                .env("ALIGNC_CACHE", project.cache_root())
                .args(["build", "main.align", "--cache-stats"]),
            &project,
            &format!("build{index}"),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if index == 1 || index == 3 {
            let diagnostics = String::from_utf8_lossy(&output.stderr);
            assert!(
                diagnostics.contains("main hit") && diagnostics.contains("main frontend hit"),
                "unchanged/reverted frontend and object must use the cache: {diagnostics}"
            );
        }
        let output = handle_command(
            &mut std::process::Command::new(project.dir.join("main")),
            &project,
            &format!("run{index}"),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, if index == 2 { b"B" } else { b"A" });
    }
}
