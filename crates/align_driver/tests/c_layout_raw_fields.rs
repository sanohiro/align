//! Concrete C records retain opaque, non-owning pointer fields through storage and imports.
#![cfg(unix)]

#[path = "helpers/ffi_aarch64.rs"]
mod ffi_aarch64;
#[path = "helpers/ffi_sysv_cases.rs"]
mod ffi_sysv_cases;
#[path = "helpers/ffi_sysv.rs"]
mod ffi_sysv;

use align_driver::{
    ArtifactStage, BuildTarget, Profile, build_per_unit, check, emit_llvm_ir, link_objects,
    lower_to_mir,
};
use align_span::SourceMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::os::unix::{
    net::{UnixListener, UnixStream},
    process::CommandExt,
};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

struct ChildGroup {
    child: Child,
    deadline: Instant,
}
impl ChildGroup {
    fn spawn(command: &mut Command) -> Self {
        Self {
            child: command
                .process_group(0)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
            deadline: Instant::now() + Duration::from_secs(60),
        }
    }
    fn tick(&self) {
        assert!(
            Instant::now() + Duration::from_secs(5) < self.deadline,
            "C-layout owner exceeded work deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    fn wait(&mut self) -> ExitStatus {
        loop {
            self.tick();
            match self.child.try_wait() {
                // Keep ownership of the group even after the leader has exited and been reaped.
                Ok(Some(status)) => return status,
                Ok(None) => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => panic!("poll C-layout owner: {error}"),
            }
        }
    }
}
impl Drop for ChildGroup {
    fn drop(&mut self) {
        let pid = i32::try_from(self.child.id()).expect("native child pid");
        loop {
            // SAFETY: this invocation owns the fresh process group, including surviving helpers.
            if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted || Instant::now() >= self.deadline {
                break;
            }
        }
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error)
                    if error.kind() == ErrorKind::Interrupted && Instant::now() < self.deadline => {
                }
                _ => {
                    eprintln!("C-layout owner failed to reap before deadline");
                    break;
                }
            }
        }
    }
}

fn helper(stage: &Path, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "c_layout_raw_helper", "--ignored", "--nocapture"])
        .env("ALIGN_C_LAYOUT_RAW_STAGE", stage)
        .env("ALIGN_C_LAYOUT_RAW_ROLE", role)
        .stdout(File::create(stage.join(format!("{role}.stdout"))).unwrap())
        .stderr(File::create(stage.join(format!("{role}.stderr"))).unwrap());
    command
}

#[test]
fn c_layout_raw_whole_and_per_unit() {
    let stage = ArtifactStage::temp("c-layout-raw").unwrap();
    let status = ChildGroup::spawn(&mut helper(stage.path(), "compile")).wait();
    assert!(
        status.success(),
        "{}\n{}",
        fs::read_to_string(stage.path().join("compile.stdout")).unwrap(),
        fs::read_to_string(stage.path().join("compile.stderr")).unwrap()
    );
}

#[test]
#[ignore = "runs only in a parent-owned process group and artifact stage"]
fn c_layout_raw_helper() {
    let stage = std::env::var_os("ALIGN_C_LAYOUT_RAW_STAGE").expect("parent-owned stage");
    let stage = Path::new(&stage);
    match std::env::var("ALIGN_C_LAYOUT_RAW_ROLE").unwrap().as_str() {
        "compile" => compile_and_run(stage),
        "leader" => {
            // Deliberately leave a live descendant after the group leader exits. Its ownership
            // remains with the parent test's ChildGroup, not with this intentionally short helper.
            helper(stage, "descendant").spawn().unwrap();
        }
        "descendant" => {
            let _stream = UnixStream::connect(stage.join("socket")).unwrap();
            loop {
                std::thread::park();
            }
        }
        _ => panic!("unknown helper role"),
    }
}

#[test]
fn c_layout_raw_group_cleanup_survives_leader_exit_and_unwind() {
    // A short root is needed for the native sockaddr_un path limit on macOS.
    let stage = ArtifactStage::in_dir(Path::new("/tmp"), "c-layout-group").unwrap();
    let listener = UnixListener::bind(stage.path().join("socket")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut owner = ChildGroup::spawn(&mut helper(stage.path(), "leader"));
    let pid = i32::try_from(owner.child.id()).unwrap();
    let (mut stream, _) = loop {
        owner.tick();
        match listener.accept() {
            Ok(stream) => break stream,
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
            Err(error) => panic!("accept descendant witness: {error}"),
        }
    };
    assert!(owner.wait().success());
    drop(owner);
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(
        stream.read(&mut [0]).unwrap(),
        0,
        "descendant must close its socket after group retirement"
    );
    let mut status = 0;
    // SAFETY: waitpid observes only our own direct child; it must already have been reaped.
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );

    let mut child_pid = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let owner = ChildGroup::spawn(Command::new("/bin/sleep").arg("60"));
        child_pid = Some(i32::try_from(owner.child.id()).unwrap());
        panic!("injected failure immediately after acquisition");
    }));
    assert!(result.is_err());
    // SAFETY: same direct-child reap observation after unwind cleanup.
    assert_eq!(
        unsafe { libc::waitpid(child_pid.unwrap(), &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

fn compile_and_run(stage: &Path) {
    let mut example_map = SourceMap::new();
    let example = check(
        &mut example_map,
        "plan137-example",
        "layout(C) NativeWindow { length: u64, data: raw, flags: u8 }\nfn main() { unsafe { window := NativeWindow { length: 0, data: raw.null(), flags: 0 } } }\n",
    );
    assert!(
        !example.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&example_map, &example.diags)
    );
    let c = r#"
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdlib.h>
struct First { void *data; uint8_t n; double f; };
struct Middle { uint8_t n; void *data; double f; };
struct Last { uint8_t n; double f; void *data; };
struct __attribute__((aligned(32))) Aligned { void *data; };
static int pointee = 42;
static unsigned char storage[128];
void *probe_data(void) { return &pointee; }
int32_t probe_pointer(void *p) { return p == &pointee && *(int *)p == 42; }
void *probe_slot(void) { memset(storage, 0x55, sizeof storage); return storage + 1; }
static int guard(size_t size) {
  if (storage[0] != 0x55) return 0;
  for (size_t i = size + 1; i < sizeof storage; ++i) if (storage[i] != 0x55) return 0;
  return 1;
}
#define ROW(TYPE) struct TYPE v; memcpy(&v,p,sizeof v); return guard(sizeof v) && v.data == &pointee && v.n == 7 && v.f == 3.5
int32_t probe_verify(void *p, int32_t kind) {
  switch(kind) {
  case 0: { ROW(First); }
  case 1: { ROW(Middle); }
  case 2: { ROW(Last); }
  case 3: { struct Aligned v; memcpy(&v,p,sizeof v); return guard(sizeof v) && v.data == &pointee; }
  default: abort();
  }
}
void probe_fill(void *p, int32_t kind) {
  switch(kind) {
  case 0: { struct First v = {&pointee,7,3.5}; memcpy(p,&v,sizeof v); return; }
  case 1: { struct Middle v = {7,&pointee,3.5}; memcpy(p,&v,sizeof v); return; }
  case 2: { struct Last v = {7,3.5,&pointee}; memcpy(p,&v,sizeof v); return; }
  case 3: { struct Aligned v = {&pointee}; memcpy(p,&v,sizeof v); return; }
  default: abort();
  }
}
struct Pair { uint64_t n; void *data; };
struct __attribute__((aligned(16))) Ptr { void *data; };
struct __attribute__((aligned(16))) Int { int64_t data; };
struct __attribute__((aligned(16))) Flt { double data; };
struct Pair probe_pair(struct Pair v, int64_t after) { if(v.n!=42 || v.data!=&pointee || after!=77) abort(); return v; }
struct Ptr probe_raw(struct Ptr v, int64_t after) { if(v.data || after!=77) abort(); return v; }
struct Int probe_int(struct Int v, int64_t after) { if(v.data!=42 || after!=77) abort(); return v; }
struct Flt probe_float(struct Flt v, int64_t after) { if(v.data!=3.5 || after!=77) abort(); return v; }
struct Ptr probe_gp(int64_t a,int64_t b,int64_t c,int64_t d,int64_t e,struct Ptr v,int64_t after) {
  if(a!=1 || b!=2 || c!=3 || d!=4 || e!=5) abort(); return probe_raw(v,after);
}
struct Flt probe_sse(double a,double b,double c,double d,double e,double f,double g,struct Flt v,int64_t after) {
  if(a!=1 || b!=2 || c!=3 || d!=4 || e!=5 || f!=6 || g!=7) abort(); return probe_float(v,after);
}
"#;
    fs::write(stage.join("probe.c"), c).unwrap();
    assert!(
        Command::new("cc")
            .args(["-c", "-O2"])
            .arg(stage.join("probe.c"))
            .arg("-o")
            .arg(stage.join("probe.o"))
            .status()
            .unwrap()
            .success()
    );
    let factory = r#"module factory
pub layout(C) First { data: raw, n: u8, f: f64 }
pub layout(C) Middle { n: u8, data: raw, f: f64 }
pub layout(C) Last { n: u8, f: f64, data: raw }
pub align(32) layout(C) Aligned { data: raw }
pub layout(C) Cell { data: raw }
pub Wrapper { cell: Cell }
pub fn keep<T>(value: T) -> T = value
pub fn paths(value: Cell, phase: i64) -> Result<Cell, Error> {
  selected := if phase == 0 { value } else { keep(value) }
  optional: Option<Cell> := if phase == 1 { None } else { Some(selected) }
  a := optional else { return Ok(value) }
  result: Result<Cell, Error> := if phase == 2 { Err(Error.Invalid) } else { Ok(a) }
  b := result.map_err(fn error: Error { error })?
  alternative: Option<Cell> := if phase == 3 { None } else { Some(b) }
  c := match alternative { Some(item) => item, None => value }
  if phase == 4 { return Ok(c) }
  mut joined := c
  mut index := 0
  loop { if index == 2 { break }; joined = keep(c); index = index + 1 }
  return Ok(joined)
}
"#;
    fs::write(stage.join("factory.align"), factory).unwrap();
    let mut source = String::from(
        r#"import factory
extern "C" fn probe_data() -> raw
extern "C" fn probe_pointer(p: raw) -> i32
extern "C" fn probe_slot() -> raw
extern "C" fn probe_verify(p: raw, kind: i32) -> i32
extern "C" fn probe_fill(p: raw, kind: i32)
fn main() -> i32 {
  unsafe {
    data := probe_data()
"#,
    );
    for (kind, name) in ["First", "Middle", "Last", "Aligned"]
        .into_iter()
        .enumerate()
    {
        let fields = if kind == 3 {
            "data: data"
        } else {
            "data: data, n: 7, f: 3.5"
        };
        source.push_str(&format!(
            r#"
    slot{kind} := probe_slot()
    raw.store(slot{kind}, 0, factory.{name} {{ {fields} }})
    if probe_verify(slot{kind}, {kind}) != 1 {{ return 10 + {kind} }}
    probe_fill(slot{kind}, {kind})
    read{kind}: factory.{name} := raw.load(slot{kind}, 0)
    if probe_pointer(read{kind}.data) != 1 {{ return 20 + {kind} }}
"#
        ));
        if kind != 3 {
            source.push_str(&format!(
                "    if read{kind}.n != 7 || read{kind}.f != 3.5 {{ return 30 + {kind} }}\n"
            ));
        }
    }
    source.push_str(
        r#"
    mut original := factory.Cell { data: data }
    copied := factory.keep(original)
    original.data = raw.null()
    if !original.data.is_null() || probe_pointer(copied.data) != 1 { return 40 }
    mut phase := 0
    loop {
      if phase == 5 { break }
      match factory.paths(copied, phase) {
        Ok(cell) => { if phase == 2 || probe_pointer(cell.data) != 1 { return 41 } },
        Err(error) => { if phase != 2 { return 42 } },
      }
      phase = phase + 1
    }
    again := factory.paths(copied, 0) else { return 43 }
    wrapped := factory.Wrapper { cell: again }
    values := [wrapped.cell, copied]
    if probe_pointer(values[0].data) != 1 || probe_pointer(values[1].data) != 1 { return 43 }
    return 0
  }
}
"#,
    );
    for per_unit in [false, true] {
        build_and_run(stage, &source, per_unit, "pointer-storage");
    }
    cached_type_round_trip(stage);
    // Independent native matrices must preserve the root fixture for later value probes.
    let root_artifacts = ["probe.o", "factory.align"];
    let original = root_artifacts.map(|name| fs::read(stage.join(name)).unwrap());
    if cfg!(all(target_arch = "aarch64", any(target_os = "linux", target_os = "macos"))) {
        ffi_aarch64::run(stage);
    }
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        ffi_sysv::run(stage);
    }
    for (name, bytes) in root_artifacts.into_iter().zip(original) {
        assert_eq!(fs::read(stage.join(name)).unwrap(), bytes, "overwrote {name}");
    }
    if cfg!(all(target_arch = "x86_64", target_os = "linux")) {
        let source = r#"
layout(C) Pair { n: u64, data: raw }
align(16) layout(C) Ptr { data: raw }
align(16) layout(C) Int { data: i64 }
align(16) layout(C) Flt { data: f64 }
extern "C" fn probe_data() -> raw
extern "C" fn probe_pointer(p: raw) -> i32
extern "C" fn probe_pair(value: Pair, after: i64) -> Pair
extern "C" fn probe_raw(value: Ptr, after: i64) -> Ptr
extern "C" fn probe_int(value: Int, after: i64) -> Int
extern "C" fn probe_float(value: Flt, after: i64) -> Flt
extern "C" fn probe_gp(a:i64,b:i64,c:i64,d:i64,e:i64,value:Ptr,after:i64) -> Ptr
extern "C" fn probe_sse(a:f64,b:f64,c:f64,d:f64,e:f64,f:f64,g:f64,value:Flt,after:i64) -> Flt
fn main() -> i32 {
  unsafe {
    pair := probe_pair(Pair { n: 42, data: probe_data() },77)
    p := probe_raw(Ptr { data: raw.null() },77)
    i := probe_int(Int { data: 42 },77)
    f := probe_float(Flt { data: 3.5 },77)
    q := probe_gp(1,2,3,4,5,Ptr { data: raw.null() },77)
    g := probe_sse(1.0,2.0,3.0,4.0,5.0,6.0,7.0,Flt { data: 3.5 },77)
    if pair.n == 42 && probe_pointer(pair.data) == 1 && p.data.is_null() && q.data.is_null() && i.data == 42 && f.data == 3.5 && g.data == 3.5 { return 0 }
    return 1
  }
}
"#;
        for per_unit in [false, true] {
            build_and_run(stage, source, per_unit, "sysv");
        }
    }
    for bad in [
        "Plain { data: raw }",
        "layout(C) Generic<T> { data: T }",
        "layout(C) Bad { data: bool }",
        "layout(C) Bad { data: char }",
        "layout(C) Bad { data: str }",
        "layout(C) Bad { data: string }",
        "layout(C) Bad { data: buffer }",
        "layout(C) Bad { data: slice<u8> }",
        "layout(C) Inner { data: raw }\nlayout(C) Bad { data: Inner }",
        "layout(C) Cell { data: raw }\nfn bad(p: raw) -> Cell = raw.load(p, 0)",
        "layout(C) Cell { data: raw }\nfn bad(p: raw, value: Cell) { raw.store(p, 0, value) }",
        "layout(C) Cell { data: raw }\nextern \"C\" fn foreign(value: Cell)\nfn bad(value: Cell) { foreign(value) }",
        "import core.json\nlayout(C) Cell { data: raw }\nfn bad(value: Cell) { json.encode(value) }",
        "import core.json\nlayout(C) Cell { data: raw }\nfn bad(text: str) -> Result<Cell, Error> = json.decode(text)",
        "layout(C) Cell { data: raw }\nfn bad(value: Cell) { arena { values := [value]; columns := values.to_soa() } }",
        "layout(C) Cell { data: raw }\nfn bad(value: Cell) { mut b: array_builder<Cell> := array_builder(); b.push(value) }",
        "layout(C) Cell { data: raw }\nfn bad(value: Cell) { arena out { mut b: array_builder<Cell> := array_builder(out); b.push(value) } }",
    ] {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, "invalid", &format!("{bad}\nfn main() {{}}\n"));
        assert!(checked.diags.has_errors(), "accepted {bad}");
    }
}

fn build_and_run(stage: &Path, source: &str, per_unit: bool, name: &str) {
    let entry = stage.join("main.align");
    fs::write(&entry, source).unwrap();
    let mut sm = SourceMap::new();
    let programs = if per_unit {
        let built = build_per_unit(&mut sm, entry.to_str().unwrap(), source);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &built.diags)
        );
        for unit in &built.units {
            let bytes = align_interface::serialize(&unit.summary);
            assert_eq!(
                bytes,
                align_interface::serialize(&align_interface::deserialize(&bytes).unwrap())
            );
        }
        built
            .units
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
    run_programs(stage, &programs, &format!("{name}-{per_unit}"), 0);
}

fn run_programs(stage: &Path, programs: &[align_mir::Program], name: &str, expected: i32) {
    let llc = align_driver::llvm_tool("llc").unwrap();
    let mut objects = vec![stage.join("probe.o")];
    for (index, mir) in programs.iter().enumerate() {
        let input = stage.join(format!("{name}-{index}.ll"));
        let object = input.with_extension("o");
        let ir = emit_llvm_ir(
            mir,
            BuildTarget::Baseline,
            Profile::Release,
            false,
            &[],
            false,
        )
        .unwrap();
        fs::write(&input, ir).unwrap();
        assert!(
            Command::new(&llc)
                .args(["-filetype=obj", "-relocation-model=pic"])
                .arg(&input)
                .arg("-o")
                .arg(&object)
                .status()
                .unwrap()
                .success()
        );
        objects.push(object);
    }
    let executable = stage.join(name);
    link_objects(
        &align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &executable,
        &[],
        Profile::Release,
    )
    .unwrap();
    assert_eq!(
        Command::new(executable).status().unwrap().code(),
        Some(expected),
        "{name}"
    );
}

fn cached_type_round_trip(stage: &Path) {
    let entry = stage.join("main.align");
    let main = "import factory\nfn main() -> i32 = factory.marker()\n";
    fs::write(&entry, main).unwrap();
    let context = align_driver::CacheContext::at(stage.join("cache"));
    let mut snapshots = Vec::new();
    for (round, integer) in [false, false, true, false].into_iter().enumerate() {
        let factory = if integer {
            "module factory\npub layout(C) Cell { data: i64 }\npub fn marker() -> i32 { value := Cell { data: 17 }; return value.data as i32 }\n"
        } else {
            "module factory\npub layout(C) Cell { data: raw }\npub fn marker() -> i32 { unsafe { value := Cell { data: raw.null() }; if value.data.is_null() { return 0 }; return 1 } }\n"
        };
        fs::write(stage.join("factory.align"), factory).unwrap();
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
        for unit in ["factory", "main"] {
            assert_eq!(
                built
                    .units
                    .iter()
                    .find(|item| item.unit == unit)
                    .unwrap()
                    .frontend
                    .as_ref()
                    .unwrap()
                    .hit,
                matches!(round, 1 | 3),
                "cache {round}/{unit}"
            );
        }
        let programs = (0..built.units.len())
            .map(|index| built.materialize(index).unwrap().clone())
            .collect::<Vec<_>>();
        let snapshot = programs
            .iter()
            .map(align_mir::print::program_to_string)
            .collect::<Vec<_>>();
        match round {
            1 | 3 => assert_eq!(snapshot, snapshots[0]),
            2 => assert_ne!(snapshot, snapshots[0]),
            _ => {}
        }
        snapshots.push(snapshot);
        run_programs(
            stage,
            &programs,
            &format!("cached-{round}"),
            if integer { 17 } else { 0 },
        );
    }
}

fn compile_c(stage: &Path, source: &str) {
    fs::write(stage.join("native_probe.c"), source).unwrap();
    assert!(
        Command::new("cc")
            .args(["-c", "-O2"])
            .arg(stage.join("native_probe.c"))
            .arg("-o")
            .arg(stage.join("probe.o"))
            .status()
            .unwrap()
            .success()
    );
}

fn cache_alignment(stage: &Path) {
    let source = "import factory\nfn main() -> i32 = factory.marker()\n";
    let entry = stage.join("main.align");
    fs::write(&entry, source).unwrap();
    let context = align_driver::CacheContext::at(stage.join("record-cache"));
    let mut snapshots = Vec::new();
    for (round, alignment) in [16, 16, 32, 16].into_iter().enumerate() {
        compile_c(
            stage,
            &format!(
                "#include <stdint.h>\nstruct __attribute__((aligned({alignment}))) Cache {{ int64_t data; }};\nstruct Cache cache_echo(struct Cache value) {{ value.data+=1; return value; }}\n"
            ),
        );
        fs::write(stage.join("factory.align"),format!("module factory\npub align({alignment}) layout(C) Cache {{ data:i64 }}\nextern \"C\" fn cache_echo(value:Cache) -> Cache\npub fn marker() -> i32 {{ unsafe {{ original := Cache {{ data:41 }}; result := cache_echo(original); if original.data != 41 || result.data != 42 {{ return 1 }}; return 0 }} }}\n")).unwrap();
        let mut sm = align_span::SourceMap::new();
        let mut built = align_driver::build_package(
            &mut sm,
            entry.to_str().unwrap(),
            source,
            &context,
            align_driver::UnitReuse::Allowed,
        );
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &built.diags)
        );
        for name in ["factory", "main"] {
            assert_eq!(
                built
                    .units
                    .iter()
                    .find(|unit| unit.unit == name)
                    .unwrap()
                    .frontend
                    .as_ref()
                    .unwrap()
                    .hit,
                matches!(round, 1 | 3),
                "{round}/{name}"
            );
        }
        let programs = (0..built.units.len())
            .map(|i| built.materialize(i).unwrap().clone())
            .collect::<Vec<_>>();
        let snapshot = programs
            .iter()
            .map(|program| {
                (
                    program
                        .structs
                        .iter()
                        .map(|record| (record.name.to_string(), record.align))
                        .collect::<Vec<_>>(),
                    align_mir::print::program_to_string(program),
                )
            })
            .collect::<Vec<_>>();
        match round {
            1 | 3 => assert_eq!(snapshot, snapshots[0]),
            2 => assert_ne!(snapshot, snapshots[0]),
            _ => {}
        }
        snapshots.push(snapshot);
        run_programs(stage, &programs, &format!("record-cache-{round}"), 0);
    }
}
