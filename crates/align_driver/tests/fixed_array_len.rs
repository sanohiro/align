//! Fixed lengths retain receiver evaluation and ownership cleanup.
mod common;
use common::*;

#[test]
fn fixed_length_receiver_control_matrix() {
    let cases = [
        ("local", "xs", "3\n0\n"),
        (
            "block",
            "{ print(\"evaluated\")\n xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "block_local",
            "{ ys := [4,5,6]\n print(\"evaluated\")\n ys }",
            "evaluated\n3\n0\n",
        ),
        (
            "unsafe_scope",
            "unsafe { print(\"evaluated\")\n xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "arena_scope",
            "arena { print(\"evaluated\")\n xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "named_arena",
            "arena scope { print(\"evaluated\")\n xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "task_scope",
            "task_group { print(\"evaluated\")\n xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "if_value",
            "if true { print(\"evaluated\")\n xs } else { xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "match_value",
            "match Some(true) { Some(flag) => { print(\"evaluated\")\n xs }, None => xs }",
            "evaluated\n3\n0\n",
        ),
        (
            "loop_value",
            "loop { print(\"evaluated\")\n break xs }",
            "evaluated\n3\n0\n",
        ),
        ("early_return", "{ if true { return 7 }\n xs }", "7\n"),
        ("literal", "[effect(1),effect(2)]", "1\n2\n2\n0\n"),
        ("bool_literal", "[true,false]", "2\n0\n"),
        ("char_literal", "['a','β']", "2\n0\n"),
        ("float_literal", "[1.5,2.5]", "2\n0\n"),
        ("str_literal", "[\"a\",\"b\"]", "2\n0\n"),
        ("generic_element", "[identity(1),identity(2)]", "2\n0\n"),
        (
            "if_false",
            "if false { print(\"wrong\")\n xs } else { print(\"selected\")\n xs }",
            "selected\n3\n0\n",
        ),
        (
            "match_none",
            "{ choice: Option<bool> := None\n match choice { Some(flag) => { print(\"wrong\")\n xs }, None => { print(\"selected\")\n xs } } }",
            "selected\n3\n0\n",
        ),
        (
            "else_operand",
            "{ choice: Option<i64> := None\n print(choice else 8)\n xs }",
            "8\n3\n0\n",
        ),
    ];
    let mut helpers = String::from(
        "module choices\nfn identity<T>(value: T) -> T = value\nfn effect(x: i64) -> i64 { print(x)\n return x }\n",
    );
    let mut main = String::from("import choices\nfn main() -> i32 {\n");
    let mut expected = String::new();
    for (name, receiver, output) in cases {
        helpers.push_str(&format!("pub fn {name}() -> i32 {{\n xs := [1,2,3]\n count := ({receiver}).len()\n print(count)\n return 0\n}}\n"));
        main.push_str(&format!(" print(\"{name}\")\n print(choices.{name}())\n"));
        expected.push_str(name);
        expected.push('\n');
        expected.push_str(output);
    }
    helpers.push_str("fn require(c: bool) -> Result<(), Error> { if c { return Ok(()) }\n return Err(Error.Invalid) }\npub fn tried(c: bool) -> Result<i64, Error> { xs := [1,2,3]\n n := ({ require(c)?\n xs }).len()\n return Ok(n) }\n");
    main.push_str(" print(choices.tried(true) else -1)\n print(choices.tried(false) else -1)\n");
    expected.push_str("3\n-1\n");
    main.push_str(" return 0\n}\n");
    let project = LenProject::new(&main, &helpers);
    for unit in [false, true] {
        let programs = project.programs(&main, unit);
        assert!(!programs.is_empty());
        if backend_available() {
            for profile in [Profile::Dev, Profile::Release] {
                assert_eq!(
                    project.run(&programs, profile),
                    expected,
                    "unit={unit}, profile={profile:?}"
                );
            }
        }
    }
}

struct LenProject(std::path::PathBuf);
impl LenProject {
    fn new(main: &str, helpers: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let project = loop {
            let path = std::env::temp_dir().join(format!(
                "align-fixed-len-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("acquire project: {error}"),
            }
        };
        std::fs::write(project.0.join("main.align"), main).unwrap();
        std::fs::write(project.0.join("choices.align"), helpers).unwrap();
        project
    }
    fn entry(&self) -> String {
        self.0.join("main.align").display().to_string()
    }
    fn programs(&self, main: &str, unit: bool) -> Vec<align_driver::MirProgram> {
        let mut sm = SourceMap::new();
        if unit {
            let checked = align_driver::build_per_unit(&mut sm, &self.entry(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            checked.units.into_iter().map(|unit| unit.mir).collect()
        } else {
            let checked = check(&mut sm, &self.entry(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            vec![align_driver::try_lower_to_mir(&checked.hir).expect("checked HIR must lower")]
        }
    }
    fn run(&self, programs: &[align_driver::MirProgram], profile: Profile) -> String {
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let object = self.0.join(format!("unit{index}.o"));
            emit_object_file(program, &object, BuildTarget::Baseline, profile, &[], false)
                .expect("codegen");
            objects.push(object);
            for library in &program.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let exe = self.0.join(format!("run{}", std::env::consts::EXE_SUFFIX));
        let refs = objects
            .iter()
            .map(std::path::PathBuf::as_path)
            .collect::<Vec<_>>();
        link_objects(
            &align_driver::CDriver::default(),
            &refs,
            &exe,
            &libraries,
            profile,
        )
        .expect("link");
        let stdout = self.0.join("stdout");
        let stderr = self.0.join("stderr");
        let mut command = std::process::Command::new(exe);
        command.stdout(std::fs::File::create(&stdout).unwrap());
        command.stderr(std::fs::File::create(&stderr).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = LenChild(Some(command.spawn().expect("spawn")));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "child deadline: {}",
                std::fs::read_to_string(&stderr).unwrap()
            );
            match child.0.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => {
                    child.0.take();
                    assert!(
                        status.success(),
                        "{status}: {}",
                        std::fs::read_to_string(stderr).unwrap()
                    );
                    return std::fs::read_to_string(stdout).unwrap();
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("wait: {error}"),
            }
        }
    }
}
impl Drop for LenProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct LenChild(Option<std::process::Child>);
impl Drop for LenChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            #[cfg(unix)]
            if let Ok(pid) = i32::try_from(child.id()) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn fixed_length_owned_receivers_keep_and_release_exact_owners() {
    let helpers = r#"module choices
extern "C" {
  fn align_rt_requested_live_bytes() -> i64
  fn align_rt_requested_live_reset()
}
Item { marker: i64, owner: buffer }
pub fn start() { unsafe { align_rt_requested_live_reset() } }
fn live() -> i64 = unsafe { align_rt_requested_live_bytes() }
fn fail() -> Result<buffer, Error> = Err(Error.Invalid)
fn partial_try() -> Result<i64, Error> {
  return Ok([Item{marker: 7, owner: buffer(4096)}, Item{marker: 7, owner: fail()?}].len())
}
fn partial_return() -> i64 {
  return [Item{marker: 7, owner: buffer(4096)}, Item{marker: 7, owner: { return 9; buffer(4096) }}].len()
}
fn literal() {
  baseline := live()
  mut count := 0
  mut i := 0
  loop {
    count = [Item{marker: 7, owner: buffer(4096)}, Item{marker: 7, owner: buffer(4096)}].len()
    if live() != baseline { print("literal-owner-leak") }
    i = i + 1
    if i == 32 { break }
  }
  print(count)
}
fn bound(c: bool) {
  xs := [Item{marker: 7, owner: buffer(4096)}, Item{marker: 7, owner: buffer(4096)}]
  ys := [Item{marker: 7, owner: buffer(4096)}, Item{marker: 7, owner: buffer(4096)}]
  baseline := live()
  if baseline <= 0 { print("probe-inactive") }
  print((if c { xs } else { ys }).len())
  print(live() == baseline)
  choice: Option<bool> := if c { Some(true) } else { None }
  print((match choice { Some(flag) => xs, None => ys }).len())
  print(live() == baseline)
  print(xs[0].marker)
  print(ys[0].marker)
}
fn scoped() {
  baseline := live()
  witness := buffer(4096)
  single := live() - baseline
  if single <= 0 { print("probe-inactive") }
  print((arena local {
    values := [Item{marker: 7, owner: buffer(4096)}]
    values
  }).len())
  print(live() == baseline + 2 * single)
  xs := [1,2]
  print(({
    mut temporary := buffer(4096)
    temporary = buffer(4096)
    xs
  }).len())
  print(live() == baseline + 3 * single)
}
pub fn exercise() {
  baseline := live()
  scoped()
  print(live() == baseline)
  bound(true)
  bound(false)
  print(live() == baseline)
  literal()
  print(live() == baseline)
  print(partial_return())
  print(live() == baseline)
  print(partial_try() else -1)
  print(live() == baseline)
}
"#;
    let main = "import choices\nfn main() { choices.start()\n choices.exercise() }\n";
    let project = LenProject::new(main, helpers);
    for unit in [false, true] {
        let programs = project.programs(main, unit);
        if backend_available() {
            for profile in [Profile::Dev, Profile::Release] {
                assert_eq!(
                    project.run(&programs, profile),
                    "1\ntrue\n2\ntrue\ntrue\n2\ntrue\n2\ntrue\n7\n7\n2\ntrue\n2\ntrue\n7\n7\ntrue\n2\ntrue\n9\ntrue\n-1\ntrue\n",
                    "unit={unit}, profile={profile:?}"
                );
            }
        }
    }
}

#[test]
fn fixed_length_preserves_array_formation_rejections() {
    for (receiver, message) in [
        ("[]", "empty array literal needs an expected element type"),
        ("[\"a\".clone()]", "cannot be an element of a fixed array"),
        ("[[1,2],[3,4]]", "composite payloads are not supported"),
    ] {
        let source = format!("fn main() -> i32 = {receiver}.len() as i32\n");
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, "fixed-len-rejection.align", &source);
        let diagnostics = align_driver::format_diagnostics(&sm, &checked.diags);
        assert!(
            checked.diags.has_errors() && diagnostics.contains(message),
            "{diagnostics}"
        );
        let mut sm = SourceMap::new();
        let units = align_driver::build_per_unit(&mut sm, "fixed-len-rejection.align", &source);
        let diagnostics = align_driver::format_diagnostics(&sm, &units.diags);
        assert!(
            units.diags.has_errors() && diagnostics.contains(message),
            "{diagnostics}"
        );
        assert!(units.units.is_empty());
    }
}
