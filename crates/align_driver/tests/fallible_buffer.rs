//! Fallible buffer construction preserves its error, owner and alignment through every stage.
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
            "fallible-buffer child exceeded work deadline"
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
                Err(error) => panic!("poll fallible-buffer child: {error}"),
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
                    eprintln!("fallible-buffer child did not reap before deadline");
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
        .replace("@align_rt_buffer_try_new(", "@probe_buffer_try_new(")
        .replace("@align_rt_buffer_try_filled(", "@probe_buffer_try_filled(")
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
fn fallible_buffer_whole_unit_control_and_cleanup() {
    assert!(backend_available() && cc_available());
    let stage = align_driver::ArtifactStage::temp("fallible-buffer").unwrap();
    std::fs::write(stage.path().join("probe.c"), r#"
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
extern int32_t align_rt_buffer_try_new(int64_t, int64_t, void **);
extern int32_t align_rt_buffer_try_filled(int64_t, uint8_t, int64_t, void **);
extern void align_rt_buffer_free(void *);
static void *live[128];
static int acquired, freed;
static void verify_cleanup(void) {
    if (acquired != 44 || freed != acquired) { fprintf(stderr, "buffer cleanup: acquired=%d freed=%d expected=44\n", acquired, freed); abort(); }
    for (int i = 0; i < 128; ++i) if (live[i]) abort();
}
static int32_t track(int32_t status, void *p) {
    if (status) { if (p) abort(); return status; }
    if (!p) abort();
    if (acquired == 0 && atexit(verify_cleanup) != 0) abort();
    ++acquired;
    for (int i = 0; i < 128; ++i) if (!live[i]) { live[i] = p; return 0; }
    abort();
}
int32_t probe_buffer_try_new(int64_t capacity, int64_t alignment, void **out) {
    if (*out) abort();
    /* ABI-only refusal proves Code(ENOMEM) mapping; native owners inject actual allocation sites. */
    if (capacity == 12345) return 17;
    int32_t status = align_rt_buffer_try_new(capacity, alignment, out);
    return track(status, *out);
}
int32_t probe_buffer_try_filled(int64_t length, uint8_t value, int64_t alignment, void **out) {
    if (*out) abort();
    if (length == 12345) return 17;
    int32_t status = align_rt_buffer_try_filled(length, value, alignment, out);
    return track(status, *out);
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
"#).unwrap();
    assert!(
        run(std::process::Command::new("cc")
            .arg("-c")
            .arg(stage.path().join("probe.c"))
            .arg("-o")
            .arg(stage.path().join("probe.o")))
        .success()
    );
    std::fs::write(stage.path().join("helper.align"), r#"module helper
pub fn make<T>(marker: T, alignment: i64) -> Result<buffer, Error> = buffer.try_new(3, alignment)
pub fn result(alignment: i64) -> Result<buffer, Error> = buffer.try_filled(3, 7, alignment)
pub fn keep(value: buffer) -> buffer = value
pub fn failure() -> Result<i64, Error> = Err(Error.Invalid)
pub fn failed() -> Result<buffer, Error> { held := buffer.try_new(2, 64)?; return buffer.try_new(1, failure()?) }
"#).unwrap();
    let source = r#"module main
import helper
extern "C" {
  fn align_buffer_probe(bytes: slice<u8>, alignment: i64) -> i32
  fn align_buffer_mark(value: i64) -> i64
  fn align_buffer_order() -> i64
}
fn aligned(borrow b: buffer, alignment: i64) -> bool = unsafe { align_buffer_probe(b.bytes(), alignment) == 1 }
fn first() -> i32 { b := buffer.try_new({ return 11; 1 }, 0); return 0 }
fn middle() -> i32 { b := buffer.try_filled(1, { return 12; 0 }, 0); return 0 }
fn last() -> i32 { b := buffer.try_filled(1, 0, { return 13; 1 }); return 0 }
fn admission(count: i64, alignment: i64) -> bool {
  return match buffer.try_new(count, alignment) { Ok(b) => false, Err(e) => match e { Invalid => true, _ => false } }
}
fn memory_failure(filled: bool) -> bool {
  attempt := if filled { buffer.try_filled(12345, 0, 64) } else { buffer.try_new(12345, 64) }
  return match attempt { Ok(b) => false, Err(e) => match e { Code(code) => code == 12, _ => false } }
}
fn control(mode: i64) -> Result<buffer, Error> {
  held := buffer.try_new(4, 64)?
  selected := if mode == 0 { buffer.try_filled(3, 7, 64)? } else { buffer.try_new(3, 64)? }
  if mode == 0 { return Ok(selected) }
  mut replacement := helper.keep(selected)
  replacement = buffer.try_filled(5, 9, 128)?
  if mode == 1 { return match buffer.try_new(-1) { Ok(other) => Ok(other), Err(e) => Ok(replacement) } }
  if mode == 2 { temporary := buffer.try_new(1)?; helper.failure()?; return Ok(temporary) }
  chosen := loop { break buffer.try_filled(2, 5, 64)? }
  return Ok(chosen)
}
fn main() -> Result<(), Error> {
  empty := buffer.try_new(0)?
  default_fill := buffer.try_filled(4, 255)?
  if empty.capacity() != 0 || empty.len() != 0 || default_fill.bytes()[0] != 255 { return Err(Error.Invalid) }
  ordered := buffer.try_filled(unsafe { align_buffer_mark(1) }, unsafe { align_buffer_mark(2) } as u8, unsafe { align_buffer_mark(4) })?
  if unsafe { align_buffer_order() } != 124 || ordered.bytes()[0] != 2 || !aligned(ordered, 4) { return Err(Error.Invalid) }
  if first() != 11 || middle() != 12 || last() != 13 { return Err(Error.Invalid) }
  if !admission(-1, 64) || !admission(0, 3) || !admission(9223372036854775807, 64) { return Err(Error.Invalid) }
  if !memory_failure(false) || !memory_failure(true) { return Err(Error.Invalid) }
  counts := [1, 2, 3].map(fn n { match buffer.try_new(n) { Ok(b) => b.capacity(), Err(e) => 0 } }).to_array()
  if counts[0] != 1 || counts[2] != 3 { return Err(Error.Invalid) }
  mut index := 0
  loop {
    if index == 4 { break }
    imported := helper.make(true, 4096)?
    if imported.capacity() != 3 || imported.len() != 0 || !aligned(imported, 4096) { return Err(Error.Invalid) }
    selected := control(index) else buffer.try_filled(6, 4, 64)?
    if !aligned(selected, 64) { return Err(Error.Invalid) }
    mapped := helper.result(64).map_err(fn e: Error { e })?
    fallback := helper.failed() else buffer.try_new(7, 64)?
    if mapped.len() != 3 || fallback.capacity() != 7 { return Err(Error.Invalid) }
    joined := match buffer.try_new(-1) { Ok(value) => value, Err(e) => buffer.try_new(2, 64)? }
    if joined.capacity() != 2 { return Err(Error.Invalid) }
    regional := arena { buffer.try_filled(3, 6, 64)? }
    if regional.bytes()[0] != 6 { return Err(Error.Invalid) }
    index = index + 1
  }
  return Ok(())
}
"#;
    for per_unit in [false, true] {
        let executable = build(
            &stage,
            source,
            per_unit,
            if per_unit { "unit" } else { "whole" },
        );
        assert!(
            run(&mut std::process::Command::new(executable)).success(),
            "unit={per_unit}"
        );
    }
}

#[test]
fn fallible_buffer_source_contract() {
    let stage = align_driver::ArtifactStage::temp("fallible-buffer-admission").unwrap();
    let entry = stage.path().join("main.align");
    for body in [
        "b := buffer.try_new()",
        "b := buffer.try_new(1, 2, 3)",
        "b := buffer.try_new(true)",
        "b := buffer.try_new(1, true)",
        "b := buffer.try_filled(1)",
        "b := buffer.try_filled(1, 0, 64, 1)",
        "b := buffer.try_filled(true, 0)",
        "b := buffer.try_filled(1, true)",
        "b := buffer.try_filled(1, 0, true)",
        "b := buffer.try_new(1, missing)",
        "b := buffer.try_new(1)?; b.try_new(2)",
        "buffer := 1; b := buffer.try_new(1)",
        "mut b := buffer.try_filled(1, 7, 64)?; view := b.bytes(); b.append_filled(1000, 0); print(view[0])",
        "b := buffer.try_filled(1, 7, 64)?; moved := consume(b); print(b.len())",
        "b := buffer.try_filled(1, 7)?; b.append_filled(1, 0)",
    ] {
        let source = format!(
            "fn consume(b: buffer) -> i64 = b.len()\nfn main() -> Result<(), Error> {{ {body}; return Ok(()) }}\n"
        );
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
    let escaping = "fn leak() -> Result<slice<u8>, Error> { b := buffer.try_filled(1, 7)?; return Ok(b.bytes()) }\nfn main() {}\n";
    std::fs::write(&entry, escaping).unwrap();
    for per_unit in [false, true] {
        let mut sm = SourceMap::new();
        let diags = if per_unit {
            build_per_unit(&mut sm, entry.to_str().unwrap(), escaping).diags
        } else {
            check(&mut sm, entry.to_str().unwrap(), escaping).diags
        };
        assert!(
            diags.has_errors(),
            "accepted escaping owner view, unit={per_unit}"
        );
        let text = align_driver::format_diagnostics(&sm, &diags);
        assert!(!text.contains("internal compiler error"), "{text}");
    }
}

#[test]
fn fallible_buffer_cache_identity() {
    let stage = align_driver::ArtifactStage::temp("fallible-buffer-cache").unwrap();
    let entry = stage.path().join("main.align");
    let helper = stage.path().join("helper.align");
    let main = "import helper\nfn main() -> Result<(), Error> { b := helper.make(true)?; print(b.capacity()); return Ok(()) }\n";
    std::fs::write(&entry, main).unwrap();
    let context = align_driver::CacheContext::at(stage.path().join("cache"));
    let mut snapshots = Vec::new();
    for (round, (alignment, filled)) in [
        (64, false),
        (64, false),
        (4096, false),
        (64, true),
        (64, false),
    ]
    .into_iter()
    .enumerate()
    {
        std::fs::write(
            &helper,
            format!(
                "module helper\npub fn make<T>(marker: T) -> Result<buffer, Error> = {}\n",
                if filled {
                    format!("buffer.try_filled(3, 0, {alignment})")
                } else {
                    format!("buffer.try_new(3, {alignment})")
                }
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
            assert_eq!(hit, matches!(round, 1 | 4), "cache round {round}/{unit}");
        }
        let snapshot: Vec<_> = (0..built.units.len())
            .map(|index| align_mir::print::program_to_string(built.materialize(index).unwrap()))
            .collect();
        match round {
            1 | 4 => assert_eq!(snapshot, snapshots[0]),
            2 | 3 => assert_ne!(snapshot, snapshots[0]),
            _ => {}
        }
        snapshots.push(snapshot);
    }
}
