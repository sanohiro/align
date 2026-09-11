//! Retained link, access and creation observations (plan 54 capability A).
mod common;
use common::*;

const HELPER: &str = r#"module helper
import std.fs
pub fn read_mode() -> fs.access_mode = fs.access_mode { read: true, write: false, execute: false }
pub fn observe<T>(borrow directory: fs.directory, marker: T) -> Result<array<u8>, Error> {
  directory.create_symlink("link", "payload")?
  target := arena { directory.read_link("link", 7)? }
  info := directory.metadata_follow("link")?
  if info.size != 5 { return Err(Error.Invalid) }
  mode := read_mode()
  if !directory.access_at("payload", mode)? { return Err(Error.Invalid) }
  if !directory.access(fs.access_mode { read: false, write: true, execute: true })? { return Err(Error.Invalid) }
  match directory.access_at("link", mode) {
    Err(error) => { match error { Invalid => {}, _ => { return Err(Error.Invalid) } } },
    _ => { return Err(Error.Invalid) },
  }
  match directory.read_link("link", 6) {
    Err(error) => { match error { Invalid => {}, _ => { return Err(Error.Invalid) } } },
    _ => { return Err(Error.Invalid) },
  }
  match directory.create_symlink("payload", "elsewhere") {
    Err(_) => {},
    Ok(_) => { return Err(Error.Invalid) },
  }
  directory.remove_file("link")?
  return Ok(target)
}
"#;

#[test]
fn observations_across_units_and_owner_expiry() {
    let fixture = private_project("fs-observations", &[], "main.align");
    let root = std::fs::canonicalize(&fixture.dir).expect("canonical fixture");
    std::fs::write(root.join("payload"),b"hello").expect("payload");
    let source = format!(r#"import std.fs
import helper
fn main() -> Result<(), Error> {{
  target := {{
    directory := fs.open_directory({:?})?
    if !directory.access(helper.read_mode())? {{ return Err(Error.Invalid) }}
    helper.observe(directory, 1)?
  }}
  print(target.len())
  print(target[0])
  return Ok(())
}}
"#, root.to_str().expect("fixture path"));
    let files = &[("helper.align",HELPER),("main.align",source.as_str())];
    let checked = diff_check_multi("fs-observations",files,"main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors,"whole:{}\nunit:{}",checked.whole_diags,checked.per_unit_diags);
    if backend_available() {
        for unit in [false,true] {
            let output = if unit { build_per_unit_multi("fs-observations-unit",files,"main.align").link_and_run() }
                else { build_and_run_multi("fs-observations-whole",files,"main.align") };
            assert_eq!(output.status.code(),Some(0),"{}",String::from_utf8_lossy(&output.stderr));
            assert_eq!(output.stdout,b"7\n112\n");
            assert_eq!(std::fs::read(root.join("payload")).expect("preserved payload"),b"hello");
        }
    }
}

#[test]
fn observation_input_domains() {
    for body in [
        "d.read_link(\"link\")",
        "d.read_link(\"link\", true)",
        "d.metadata_follow(12)",
        "d.access(1)",
        "d.access(fs.access_mode { read: true, write: false })",
        "d.access_at(\"x\", true)",
        "d.create_symlink(\"x\", 1)",
    ] {
        let source = format!("import std.fs\nfn main() -> Result<(), Error> {{ d := fs.open_directory(\".\")?; value := {body}; return Ok(()) }}\n");
        let checked = diff_check_multi("fs-observation-invalid", &[("main.align",&source)],"main.align");
        assert!(checked.whole_errors && checked.per_unit_errors,"accepted {body}");
    }
    let no_import = "fn take(mode: fs.access_mode) {}\nfn main() {}\n";
    let checked = diff_check_multi("fs-access-import",&[("main.align",no_import)],"main.align");
    assert!(checked.whole_errors && checked.per_unit_errors);
}

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

