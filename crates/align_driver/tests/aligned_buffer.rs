//! Explicit payload alignment reaches the native allocation through every compiler stage.
mod common;
use common::*;
use std::io::ErrorKind;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct ChildOwner {
    child: Option<std::process::Child>,
    deadline: Instant,
}
impl ChildOwner {
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "aligned-buffer child exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn wait(&mut self) -> std::process::ExitStatus {
        loop {
            self.tick();
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    return status;
                }
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll aligned-buffer child: {error}"),
            }
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        // Compiler drivers can launch helpers. Every invocation owns a fresh process group.
        loop {
            let result = unsafe { libc::kill(-i32::try_from(child.id()).unwrap(), libc::SIGKILL) };
            let result = if result == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            };
            match result {
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => break,
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("aligned-buffer child did not reap before deadline");
                    return;
                }
            }
        }
    }
}

fn run(command: &mut std::process::Command) -> std::process::ExitStatus {
    let mut owner = ChildOwner {
        child: Some(
            command
                .process_group(0)
                .stdin(std::process::Stdio::null())
                .spawn()
                .expect("spawn owner command"),
        ),
        deadline: Instant::now() + Duration::from_secs(30),
    };
    owner.wait()
}

fn build(stage: &align_driver::ArtifactStage, source: &str, per_unit: bool, name: &str) -> PathBuf {
    let entry = stage.path().join("main.align");
    std::fs::write(&entry, source).unwrap();
    let mut sm = SourceMap::new();
    let programs = if per_unit {
        let walk = build_per_unit(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !walk.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &walk.diags)
        );
        for unit in &walk.units {
            let bytes = align_interface::serialize(&unit.summary);
            let replayed = align_interface::deserialize(&bytes).unwrap();
            assert_eq!(bytes, align_interface::serialize(&replayed));
        }
        walk.units
            .into_iter()
            .map(|unit| unit.mir)
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &checked.diags)
        );
        vec![lower_to_mir(&checked.hir)]
    };
    let mut objects = vec![stage.path().join("probe.o")];
    let mut libraries = Vec::new();
    let llc = align_driver::llvm_tool("llc").expect("matched LLVM llc");
    for (index, mir) in programs.iter().enumerate() {
        let object = stage.path().join(format!("{name}-{index}.o"));
        // Count generated ownership transitions while delegating to the real runtime. The C
        // ledger rejects duplicate frees before they reach the allocator and checks leaks at exit.
        let ir = emit_llvm_ir(
            mir,
            BuildTarget::Baseline,
            Profile::Release,
            false,
            &[],
            false,
        )
        .unwrap()
        .replace("@align_rt_buffer_new(", "@probe_buffer_new(")
        .replace("@align_rt_buffer_filled(", "@probe_buffer_filled(")
        .replace("@align_rt_buffer_free(", "@probe_buffer_free(");
        let input = stage.path().join(format!("{name}-{index}.ll"));
        std::fs::write(&input, ir).unwrap();
        assert!(
            run(std::process::Command::new(&llc)
                .args(["-filetype=obj", "-relocation-model=pic"])
                .arg(input)
                .arg("-o")
                .arg(&object))
            .success()
        );
        objects.push(object);
        for library in &mir.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let executable = stage.path().join(name);
    let refs = objects
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    link_objects(
        &align_driver::CDriver::default(),
        &refs,
        &executable,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    executable
}

#[test]
fn aligned_buffer_whole_unit_imports_growth_and_control() {
    assert!(backend_available() && cc_available());
    let stage = align_driver::ArtifactStage::temp("aligned-buffer").unwrap();
    std::fs::write(
        stage.path().join("probe.c"),
        r#"
#include <stdint.h>
#include <stdlib.h>
extern void *align_rt_buffer_new(int64_t, int64_t, int32_t);
extern void *align_rt_buffer_filled(int64_t, uint8_t, int64_t, int32_t);
extern void align_rt_buffer_free(void *);
static void *live[128];
static int acquired, freed;
static void verify_cleanup(void) {
    if (acquired != 54 || freed != acquired) abort();
    for (int i = 0; i < 128; ++i) if (live[i]) abort();
}
static void *track(void *p) {
    if (!p) abort();
    if (acquired == 0 && atexit(verify_cleanup) != 0) abort();
    ++acquired;
    for (int i = 0; i < 128; ++i) if (!live[i]) { live[i] = p; return p; }
    abort();
}
void *probe_buffer_new(int64_t capacity, int64_t alignment, int32_t pages) {
    return track(align_rt_buffer_new(capacity, alignment, pages));
}
void *probe_buffer_filled(int64_t length, uint8_t value, int64_t alignment, int32_t pages) {
    return track(align_rt_buffer_filled(length, value, alignment, pages));
}
void probe_buffer_free(void *p) {
    if (!p) return;
    for (int i = 0; i < 128; ++i) if (live[i] == p) {
        live[i] = 0; ++freed; align_rt_buffer_free(p); return;
    }
    abort();
}
int32_t align_buffer_probe(const unsigned char *bytes, int64_t alignment) {
    return bytes != 0 && (uintptr_t)bytes % (uint64_t)alignment == 0;
}
static int64_t order;
int64_t align_buffer_mark(int64_t value) { order = order * 10 + value; return value; }
int64_t align_buffer_order(void) { return order; }
"#,
    )
    .unwrap();
    assert!(
        run(std::process::Command::new("cc")
            .arg("-c")
            .arg(stage.path().join("probe.c"))
            .arg("-o")
            .arg(stage.path().join("probe.o")))
        .success()
    );
    std::fs::write(
        stage.path().join("helper.align"),
        r#"module helper
pub fn make<T>(marker: T, alignment: i64) -> buffer = buffer(3, alignment)
pub fn result(alignment: i64) -> Result<buffer, Error> = Ok(buffer.filled(3, 7, alignment))
pub fn keep(value: buffer) -> buffer = value
pub fn failure() -> Result<i64, Error> = Err(Error.Invalid)
pub fn failed() -> Result<buffer, Error> { held := buffer(2, 64); return Ok(buffer(1, failure()?)) }
"#,
    )
    .unwrap();
    std::fs::write(stage.path().join("input"), b"abcdefghi\n").unwrap();
    let path = stage
        .path()
        .join("input")
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = r#"module main
import helper
import std.fs
extern "C" {
  fn align_buffer_probe(bytes: slice<u8>, alignment: i64) -> i32
  fn align_buffer_mark(value: i64) -> i64
  fn align_buffer_order() -> i64
}
fn aligned(borrow b: buffer, alignment: i64) -> bool = unsafe { align_buffer_probe(b.bytes(), alignment) == 1 }
fn first() -> i32 { b := buffer({ return 11; 1 }, 0); return 0 }
fn middle() -> i32 { b := buffer.filled(1, { return 12; 0 }, 0); return 0 }
fn last() -> i32 { b := buffer.filled(1, 0, { return 13; 1 }); return 0 }
fn keep_error(error: Error) -> Error = error
fn main() -> i32 {
  empty := buffer(0, 16384)
  if !aligned(empty, 16384) { return 1 }
  failed_hint := buffer(9223372036854775807, 4096)
  if failed_hint.capacity() != 0 || !aligned(failed_hint, 4096) { return 2 }
  f := fs.open_ro("INPUT") else { return 3 }
  mut count := 0
  loop {
    if count >= 12 { break }
    mut b := helper.make(true, 4096)
    if b.len() != 0 || b.capacity() != 3 || !aligned(b, 4096) { return 4 }
    b.put_u64_le(72623859790382856)
    b.append("abc")
    b.append_filled(9000, 42)
    if !aligned(b, 4096) || b.bytes()[8] != 97 || b.bytes()[9000] != 42 { return 5 }
    moved := helper.keep(b)
    if !aligned(moved, 4096) || moved.len() != 9011 { return 6 }
    selected := if count == 0 { helper.result(64) } else { helper.result(4096) }
    mut replaced := selected.map_err(keep_error) else { return 7 }
    replaced = buffer.filled(12, 45, 16384)
    if !aligned(replaced, 16384) { return 8 }
    n := f.pread_into(replaced, 2, 4, 0) else { return 9 }
    if n != 4 || replaced.bytes()[2] != 97 || replaced.bytes()[6] != 45 || !aligned(replaced, 16384) { return 10 }
    from_match := match helper.result(64) { Ok(value) => value, Err(error) => { return 14 } }
    if !aligned(from_match, 64) { return 15 }
    count = count + 1
  }
  raw_reader := fs.open("INPUT") else { return 16 }
  reader := raw_reader.buffered()
  mut line := buffer(1, 4096)
  read := reader.read_line(line) else { return 17 }
  if read != 10 || line.len() != 9 || !aligned(line, 4096) { return 18 }
  if first() != 11 || middle() != 12 || last() != 13 { return 19 }
  match helper.failed() { Ok(value) => { return 20 }, Err(error) => {} }
  ordered := unsafe { buffer.filled(align_buffer_mark(2), align_buffer_mark(3) as u8, align_buffer_mark(4)) }
  if !aligned(ordered, 4) || ordered.len() != 2 || ordered.bytes()[1] != 3 { return 21 }
  unsafe { if align_buffer_order() != 234 { return 22 } }
  plain := buffer.filled(1, 7)
  if plain.bytes()[0] != 7 { return 23 }
  return 0
}
"#.replace("INPUT", &path);
    for per_unit in [false, true] {
        let executable = build(&stage, &source, per_unit, &format!("aligned-{per_unit}"));
        assert_eq!(
            run(&mut std::process::Command::new(executable)).code(),
            Some(0)
        );
    }
}

#[test]
fn aligned_buffer_source_rejects_invalid_types_and_stale_views() {
    let stage = align_driver::ArtifactStage::temp("aligned-buffer-rejection").unwrap();
    let entry = stage.path().join("main.align");
    for body in [
        "b := buffer()",
        "b := buffer(1, 2, 3)",
        "b := buffer(true, 64)",
        "b := buffer(1, true)",
        "b := buffer.filled(1)",
        "b := buffer.filled(1, 0, 64, 1)",
        "b := buffer.filled(1, 0, true)",
        "b := buffer.filled(1, true, 64)",
        "b := buffer(1, missing)",
        "mut b := buffer.filled(1, 7, 64); view := b.bytes(); b.append_filled(1000, 0); print(view[0])",
        "b := buffer.filled(1, 7, 64); moved := consume(b); print(b.len())",
        "b := buffer.filled(1, 7, 64); moved := buffer(1, consume(b)); print(b.len())",
    ] {
        let source = format!("fn consume(b: buffer) -> i64 = b.len()\nfn main() {{ {body} }}\n");
        std::fs::write(&entry, &source).unwrap();
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                build_per_unit(&mut sm, entry.to_str().unwrap(), &source).diags
            } else {
                check(&mut sm, entry.to_str().unwrap(), &source).diags
            };
            assert!(diags.has_errors(), "accepted {body}, unit={per_unit}");
            let text = align_driver::format_diagnostics(&sm, &diags);
            assert!(!text.contains("internal compiler error"), "{text}");
        }
    }
}

#[test]
fn aligned_buffer_stack_promotion_requires_default_alignment() {
    for (argument, promoted) in [
        ("", true),
        (", 1", true),
        (", 1, buffer.page_policy.Default", true),
        (", 1, buffer.page_policy.PreferHuge", false),
        (", 64", false),
        (", 0", false),
        (", alignment", false),
    ] {
        let source = format!(
            "fn value(alignment: i64) -> u32 {{ mut b := buffer(4{argument}); b.put_u32_le(7); return b.bytes().u32_le(0) }}\n"
        );
        let ir = emit_llvm(&source);
        assert_eq!(
            !ir.contains("call ptr @align_rt_buffer_new"),
            promoted,
            "{argument}: {ir}"
        );
    }
}

#[test]
fn aligned_buffer_generic_alignment_survives_cache_edit_and_restore() {
    let stage = align_driver::ArtifactStage::temp("aligned-buffer-cache").unwrap();
    let entry = stage.path().join("main.align");
    let helper = stage.path().join("helper.align");
    let main = "import helper\nfn main() { b := helper.make(true); print(b.capacity()) }\n";
    std::fs::write(&entry, main).unwrap();
    let context = align_driver::CacheContext::at(stage.path().join("cache"));
    let mut snapshots = Vec::new();
    for (round, alignment) in [64, 64, 4096, 64].into_iter().enumerate() {
        std::fs::write(
            &helper,
            format!(
                "module helper\npub fn make<T>(marker: T) -> buffer = buffer(3, {alignment})\n"
            ),
        )
        .unwrap();
        let mut sm = SourceMap::new();
        let mut built = align_driver::build_package(
            &mut sm,
            entry.to_str().unwrap(),
            main,
            &context,
            align_driver::UnitReuse::Allowed,
        );
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &built.diags)
        );
        for unit in ["helper", "main"] {
            let hit = built
                .units
                .iter()
                .find(|item| item.unit == unit)
                .unwrap()
                .frontend
                .as_ref()
                .unwrap()
                .hit;
            assert_eq!(hit, matches!(round, 1 | 3), "cache round {round}/{unit}");
        }
        let snapshot: Vec<_> = (0..built.units.len())
            .map(|index| align_mir::print::program_to_string(built.materialize(index).unwrap()))
            .collect();
        match round {
            1 | 3 => assert_eq!(snapshot, snapshots[0]),
            2 => assert_ne!(snapshot, snapshots[0]),
            _ => {}
        }
        snapshots.push(snapshot);
    }
}
