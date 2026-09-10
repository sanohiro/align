//! Retained directory ownership and raw entry operations, plan 45.
mod common;
use common::*;

#[test]
fn retained_tree_operations_round_trip() {
    if !backend_available() {
        return;
    }
    let fixture = private_project("roundtrip", &[], "main.align");
    let root = fixture.dir.clone();
    let source = r#"
import std.fs
pub fn main(args: array<str>) -> Result<(), Error> {
  directory := fs.open_directory(args[1])?
  directory.set_mode(448)?
  directory.create_dir("child", 448)?
  child := directory.open_dir("child")?
  writer := child.create_new("payload")?
  writer.write("hello")?
  writer.flush()?
  writer.set_mode(384)?
  wm := writer.metadata()?
  print(wm.size)
  reader := child.open_read("payload")?
  reader.set_mode(384)?
  rm := reader.metadata()?
  print(rm.size)
  single := child.open_read_single_link("payload")?
  sm := single.metadata()?
  print(sm.links)
  pm := child.metadata_at("payload")?
  print(pm.size)
  dm := child.metadata()?
  print(dm.mode)
  cursor := child.cursor()?
  entry := cursor.next()? else { return Ok(()) }
  print(entry.name.len())
  match cursor.next()? { Some(extra) => print(99), None => print(0) }
  child.remove_file("payload")?
  directory.remove_dir("child")?
  return Ok(())
}
"#;
    let out = build_and_run_args(
        "retained-tree-roundtrip",
        source,
        &[root.to_str().expect("fixture UTF-8")],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "5\n5\n1\n5\n448\n7\n0\n"
    );
}

#[test]
fn carriers_and_imported_generics() {
    let helper = r#"module helper
import std.fs
pub Holder { directory: fs.directory }
pub fn open<T>(path: str, marker: T) -> Result<Holder, Error> {
  directory := fs.open_directory(path)?
  return Ok(Holder { directory: directory })
}
pub fn cursor(holder: Holder) -> Result<fs.dir_cursor, Error> {
  directory := holder.directory
  return directory.cursor()
}
pub fn advance(borrow mut cursor: fs.dir_cursor) -> Result<Option<fs.dir_entry>, Error> = cursor.next()
"#;
    let main = r#"import std.fs
import helper
fn main() -> Result<(), Error> {
  holder := helper.open(".", 1)
  owned := holder?
  mut cursor := helper.cursor(owned)?
  item := helper.advance(cursor)?
  match item { Some(entry) => print(entry.name.len() > 0), None => print(false) }
  return Ok(())
}
"#;
    let files = &[("helper.align", helper), ("main.align", main)];
    let checked = diff_check_multi("retained-carriers", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:{}\nunit:{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let whole = build_and_run_multi("retained-carriers-whole", files, "main.align");
        let built = build_per_unit_multi("retained-carriers-unit", files, "main.align");
        assert!(
            !built
                .link_libs_union()
                .iter()
                .any(|library| library == "crypto")
        );
        let unit = built.link_and_run();
        assert_eq!(
            whole.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&whole.stderr)
        );
        assert_eq!(
            unit.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&unit.stderr)
        );
        assert_eq!(whole.stdout, b"true\n");
        assert_eq!(whole.stdout, unit.stdout);
    }
}

#[test]
fn rejects_wrong_inputs_and_receiver_ownership() {
    for (name, source) in [
        ("missing-import", "fn main() = fs.open_directory(\".\")"),
        (
            "wrong-root",
            "import std.fs\nfn main() = fs.open_directory(1)",
        ),
        (
            "wrong-mode",
            "import std.fs\nfn f(borrow d: fs.directory) = d.set_mode(true)\nfn main() {}",
        ),
        (
            "shared-next",
            "import std.fs\nfn f(borrow c: fs.dir_cursor) = c.next()\nfn main() {}",
        ),
        (
            "temporary",
            "import std.fs\nfn main() -> Result<(), Error> { c := fs.open_directory(\".\")?.cursor()?; return Ok(()) }",
        ),
        (
            "use-after-move",
            "import std.fs\nfn take(d: fs.directory) {}\nfn main() -> Result<(), Error> { d := fs.open_directory(\".\")?; take(d); d.metadata()?; return Ok(()) }",
        ),
        (
            "eager-move",
            "import std.fs\nfn take(d: fs.directory) -> str = \"x\"\nfn main() -> Result<(), Error> { d := fs.open_directory(\".\")?; d.remove_file(take(d))?; return Ok(()) }",
        ),
        (
            "direct-array",
            "import std.fs\nfn main() -> Result<(), Error> { d := fs.open_directory(\".\")?; values := [d]; return Ok(()) }",
        ),
    ] {
        assert!(check_errs(name, source), "{name} unexpectedly accepted");
    }
}

const _: extern "C" fn() -> i64 = align_runtime::align_rt_alloc_count;
const _: extern "C" fn() -> i64 = align_runtime::align_rt_free_count;
const CLEANUP_HELPER: &str = r#"module helper
import std.fs
pub Holder { directory: fs.directory }
CursorHolder { cursor: fs.dir_cursor }
Choice { Held(fs.directory), Empty }
fn forward<T>(value: T) -> T = value
fn changed(error: Error) -> Error = error
fn early_root() -> Result<(), Error> {
  unused := fs.open_directory({ return Ok(()); "." })?
  return Err(Error.Invalid)
}
fn early_path(directory: fs.directory) -> Result<(), Error> {
  directory.remove_file({ return Ok(()); "../invalid" })?
  return Err(Error.Invalid)
}
fn early_mode(directory: fs.directory) -> Result<(), Error> {
  directory.create_dir("../invalid", { return Ok(()); 448 })?
  return Err(Error.Invalid)
}
fn early() -> Result<(), Error> {
  directory := fs.open_directory(".")?
  cursor := directory.cursor()?
  entry := cursor.next()? else { return Err(Error.Invalid) }
  if entry.name.len() == 0 { return Err(Error.Invalid) }
  return Err(Error.Invalid)
}
pub fn exercise() -> Result<(), Error> {
  early_root()?
  early_path(fs.open_directory(".")?)?
  early_mode(fs.open_directory(".")?)?
  mut directory := fs.open_directory(".")?
  directory = forward(fs.open_directory(".")?)
  held := Holder { directory: directory }
  extracted := held.directory
  optional: Option<fs.directory> := Some(extracted)
  owned := optional else { return Err(Error.Invalid) }
  selected := if true { fs.open_directory(".")? } else { fs.open_directory(".")? }
  fixed := [Holder { directory: fs.open_directory(".")? }]
  mut builder: array_builder<CursorHolder> := array_builder()
  builder.push(CursorHolder { cursor: owned.cursor()? })
  cursor_rows := builder.build()
  choice := Choice.Held(fs.open_directory(".")?)
  match choice { Held(moved) => { observed := moved.metadata()? }, Empty => {} }
  match early().map_err(changed) { Ok(_) => {}, Err(_) => {} }
  loop { temporary := fs.open_directory(".")?; break }
  return Ok(())
}
"#;

#[test]
fn ownership_cleanup_and_negative_controls() {
    // ArrayBuilder transfers one native allocation outside alloc_count; free_count includes it.
    // Requested-live bytes and native descriptor counts are the actual leak oracles.
    let main = r#"import helper
extern "C" fn retained_fd_count() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  before_fd := unsafe { retained_fd_count() }
  before_alloc := unsafe { align_rt_alloc_count() }
  before_free := unsafe { align_rt_free_count() }
  helper.exercise() else { return 2 }
  allocated := unsafe { align_rt_alloc_count() } - before_alloc
  freed := unsafe { align_rt_free_count() } - before_free
  after_fd := unsafe { retained_fd_count() }
  if before_fd < 0 || after_fd < 0 || allocated < 1 { return 3 }
  if before_fd != after_fd || unsafe { align_rt_requested_live_bytes() } != 0 { return 1 }
  if freed != allocated + 1 { return 1 }
  return 0
}
"#;
    let checked = diff_check_multi(
        "retained-cleanup",
        &[("helper.align", CLEANUP_HELPER), ("main.align", main)],
        "main.align",
    );
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:{} unit:{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for per_unit in [false, true] {
            for omit in [None, Some("directory"), Some("cursor"), Some("entry")] {
                let out = run_retained_cleanup_probe(main, per_unit, omit);
                assert_eq!(
                    out.status.code(),
                    Some(i32::from(omit.is_some())),
                    "per_unit={per_unit} omit={omit:?}: {}",
                    String::from_utf8_lossy(&out.stdout)
                );
            }
        }
    }
}

fn run_retained_cleanup_probe(
    main: &str,
    per_unit: bool,
    omit: Option<&str>,
) -> std::process::Output {
    let project = private_project(
        "retained-cleanup-probe",
        &[("helper.align", CLEANUP_HELPER), ("main.align", main)],
        "main.align",
    );
    let entry = project.dir.join("main.align");
    let mut map = SourceMap::new();
    let mut programs = if per_unit {
        let walk = build_per_unit(&mut map, &entry.display().to_string(), main);
        assert!(
            !walk.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&map, &walk.diags)
        );
        walk.units
            .into_iter()
            .map(|unit| unit.mir)
            .collect::<Vec<_>>()
    } else {
        let checked = check(&mut map, &entry.display().to_string(), main);
        assert!(!checked.diags.has_errors());
        vec![lower_to_mir(&checked.hir)]
    };
    if let Some(omit) = omit {
        let mut removed = 0;
        for program in &mut programs {
            for function in &mut program.fns {
                if !function.name.as_str().ends_with("$early") {
                    continue;
                }
                for block in &mut function.blocks {
                    block.stmts.retain(|statement| {
                        let remove = matches!(statement, align_mir::Stmt::Drop(slot) if match (omit, function.slots[*slot as usize]) {
                            ("directory", align_sema::Ty::FsDirectory) | ("cursor", align_sema::Ty::FsDirCursor) => true,
                            ("entry", align_sema::Ty::Struct(id)) => program.structs[id as usize].name == "fs.dir_entry",
                            _ => false,
                        });
                        if remove { removed += 1; }
                        !remove
                    });
                    block.stmt_lines.clear();
                }
            }
        }
        assert!(
            removed > 0,
            "negative control removed no retained owner Drop"
        );
    }
    let c_source = project.dir.join("probe.c");
    let c_object = project.dir.join("probe.o");
    std::fs::write(
        &c_source,
        r#"
#include <dirent.h>
#include <stdint.h>
#include <string.h>
int64_t retained_fd_count(void) {
    DIR *directory = opendir("/dev/fd");
    if (!directory) return -1;
    int64_t count = 0;
    struct dirent *entry;
    while ((entry = readdir(directory))) {
        if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) ++count;
    }
    closedir(directory);
    return count;
}
"#,
    )
    .expect("write fd oracle");
    let status = std::process::Command::new("cc")
        .args(["-c"])
        .arg(&c_source)
        .arg("-o")
        .arg(&c_object)
        .status()
        .expect("compile fd oracle");
    assert!(status.success());
    let mut objects = vec![c_object];
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = project.dir.join(format!("unit{index}.o"));
        emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .expect("emit retained probe");
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let executable = project.dir.join("probe");
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
    .expect("link retained probe");
    std::process::Command::new(executable)
        .output()
        .expect("run retained probe")
}

#[test]
fn metadata_matches_native_fields() {
    use std::os::unix::fs::MetadataExt;
    if !backend_available() {
        return;
    }
    let fixture = private_project("metadata", &[], "main.align");
    let path = fixture.dir.join("payload");
    std::fs::write(&path, b"metadata").expect("fixture file");
    let source = r#"import std.fs
fn main(args: array<str>) -> Result<(), Error> {
  file := fs.open_rw(args[1])?
  file.set_mode(384)?
  metadata := file.metadata()?
  print(match metadata.kind { Regular => 0, Directory => 1, Symlink => 2, Other => 3 })
  print(metadata.device); print(metadata.inode); print(metadata.links); print(metadata.mode); print(metadata.size)
  print(metadata.modified_seconds); print(metadata.modified_nanoseconds)
  print(metadata.changed_seconds); print(metadata.changed_nanoseconds)
  return Ok(())
}
"#;
    let out = build_and_run_args(
        "retained-metadata-oracle",
        source,
        &[path.to_str().expect("fixture UTF-8")],
    );
    let native = std::fs::metadata(&path).expect("native metadata");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = format!(
        "0\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        native.dev(),
        native.ino(),
        native.nlink(),
        native.mode() & 0o7777,
        native.size(),
        native.mtime(),
        native.mtime_nsec(),
        native.ctime(),
        native.ctime_nsec()
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}

#[test]
fn interfaces_and_cache() {
    if !backend_available() {
        return;
    }
    let helper = "module helper\nimport std.fs\npub fn open() -> Result<fs.directory, Error> = fs.open_directory(\".\")\n";
    let main = "import helper\nimport std.fs\nfn main() -> Result<(), Error> { directory := helper.open()?; cursor := directory.cursor()?; entry := cursor.next()?; return Ok(()) }\n";
    let project = private_project(
        "retained-cache",
        &[("helper.align", helper), ("main.align", main)],
        "main.align",
    );
    let cache = project.cache();
    assert!(thin_build(&project, &cache, 1).all_miss());
    assert!(thin_build(&project, &cache, 1).all_hit());
    project.write(
        "helper.align",
        &helper.replace("fs.open_directory(\".\")", "fs.open_directory(\"/\")"),
    );
    assert!(!thin_build(&project, &cache, 1).all_hit());
    project.write("helper.align", helper);
    assert!(thin_build(&project, &cache, 1).all_hit());
}

/// Acquire a private directory exclusively before arming the existing project cleanup owner.
/// Canonicalizing this acquired root keeps fixture setup separate from no-follow path admission.
fn private_project(tag: &str, files: &[(&str, &str)], entry: &str) -> Proj {
    use std::os::unix::fs::DirBuilderExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("fixture clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "align-retained-private-{}-{time}-{nonce}-{tag}",
        std::process::id()
    ));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .expect("acquire private fixture");
    let mut project = Proj {
        dir,
        entry: entry.to_string(),
    };
    project.dir = std::fs::canonicalize(&project.dir).expect("canonicalize acquired fixture");
    for (name, source) in files {
        project.write(name, source);
    }
    project
}

#[test]
fn opaque_sum_collection_exclusions() {
    for owner in ["fs.directory", "fs.dir_cursor"] {
        for container in ["slice<Choice>", "array<Choice>", "array_builder<Choice>"] {
            let source = format!(
                "import std.fs\nChoice {{ Held({owner}), Empty }}\nfn inspect(values: {container}) {{}}\nfn main() {{}}\n"
            );
            let copy_source = source.replace(&format!("Held({owner})"), "Held(i64)");
            assert!(
                !check_errs("retained-copy-sum-control", &copy_source),
                "invalid fixture syntax: {container}"
            );
            let checked = diff_check_multi(
                "retained-sum-exclusion",
                &[("main.align", &source)],
                "main.align",
            );
            assert!(
                checked.whole_errors && checked.per_unit_errors,
                "{owner} {container} unexpectedly admitted: {} / {}",
                checked.whole_diags,
                checked.per_unit_diags
            );
        }
        let source = format!(
            "import std.fs\nChoice {{ Held({owner}), Empty }}\nfn generic<T>(values: slice<T>) {{}}\nfn inspect(value: Choice) {{ values := [value]; generic(values) }}\nfn main() {{}}\n"
        );
        assert!(check_errs("retained-generic-sum-exclusion", &source));
    }
}
