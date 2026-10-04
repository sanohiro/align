//! Advisory RAM observations use ordinary scalar Result transport and impurity.
mod common;
use common::*;

const HELPER: &str = r#"module helper
import std.os
fn forward<T>(value: T) -> T = value
fn keep(error: Error) -> Error = error
pub fn observe<T>(available: bool, marker: T) -> Result<i64, Error> = arena {
  if available { os.available_memory() } else { os.physical_memory() }
}
pub fn exercise() -> Result<(), Error> {
  original := forward(os.physical_memory())
  copied := original
  count := copied.map_err(keep)?
  mut replaced := count
  replaced = os.physical_memory()?
  optional := Some(replaced)
  last := optional else { return Err(Error.Invalid) }
  selected := if last > 0 { os.available_memory() } else { os.physical_memory() }
  match selected { Ok(value) => { if value < 0 { return Err(Error.Invalid) } }, Err(error) => { return Err(error) } }
  loop { available := os.available_memory()?; if available < 0 { return Err(Error.Invalid) }; break }
  fallback := os.available_memory() else { return Err(Error.Invalid) }
  if count <= 0 || last <= 0 || fallback < 0 { return Err(Error.Invalid) }
  return Ok(())
}
"#;
const MAIN: &str = r#"import std.os
import helper
fn main() -> Result<(), Error> {
  helper.exercise()?
  print(helper.observe(false, 1)?)
  print(helper.observe(true, true)?)
  print(true)
  return Ok(())
}
"#;

#[test]
fn memory_native_calls_use_i32_status_and_i64_output() {
    if !backend_available() { return; }
    let ir = emit_llvm("import std.os\nfn main() -> Result<(), Error> { total := os.physical_memory()?; available := os.available_memory()?; print(total); print(available); return Ok(()) }\n");
    for symbol in ["physical_memory", "available_memory"] {
        let needle = format!("call i32 @align_rt_os_{symbol}(ptr ");
        let calls: Vec<_> = ir.lines().filter_map(|line| line.split_once(&needle)).collect();
        assert_eq!(calls.len(), 1, "{symbol}: {ir}");
        let pointer = calls[0].1.split(')').next().unwrap();
        assert!(ir.contains(&format!("{pointer} = alloca i64, align 8")), "{symbol}: {ir}");
    }
}

fn qualify(output: &[u8]) {
    let text = core::str::from_utf8(output).expect("scalar output UTF-8");
    let fields: Vec<_> = text.lines().collect();
    assert_eq!(fields.len(), 3, "{text}");
    assert_eq!(fields[2], "true");
    let total: i64 = fields[0].parse().expect("total");
    let available: i64 = fields[1].parse().expect("available");
    assert!(total > 0 && available >= 0, "{text}");
    #[cfg(target_os = "linux")]
    {
        let native = std::fs::read_to_string("/proc/meminfo").unwrap();
        let count: i64 = native
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(total, count * 1024);
        assert_eq!(available % 1024, 0);
    }
    #[cfg(target_os = "macos")]
    {
        let mut native = 0u64;
        let mut length = core::mem::size_of::<u64>();
        assert_eq!(
            unsafe {
                libc::sysctlbyname(
                    c"hw.memsize".as_ptr(),
                    (&mut native as *mut u64).cast(),
                    &mut length,
                    core::ptr::null_mut(),
                    0,
                )
            },
            0
        );
        assert_eq!(length, 8);
        assert_eq!(u64::try_from(total).unwrap(), native);
    }
}

#[test]
fn memory_observation_transport_and_cached_interfaces() {
    let files = &[("helper.align", HELPER), ("main.align", MAIN)];
    let checked = diff_check_multi("os-memory", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:{}\nunit:{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for per_unit in [false, true] {
            let output = if per_unit {
                build_per_unit_multi("memory-unit", files, "main.align").link_and_run()
            } else {
                build_and_run_multi("memory-whole", files, "main.align")
            };
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            qualify(&output.stdout);
        }
        let project = Proj::new("memory-cache", files, "main.align");
        let cache = project.cache();
        let cold = thin_build(&project, &cache, 1);
        assert!(cold.all_miss());
        let cold_objects: Vec<_> = cold
            .objs
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        qualify(cold.run(&project).as_bytes());
        let hot = thin_build(&project, &cache, 1);
        assert!(hot.all_hit());
        for (path, expected) in hot.objs.iter().zip(cold_objects) {
            assert_eq!(std::fs::read(path).unwrap(), expected);
        }
        qualify(hot.run(&project).as_bytes());
    }
}

#[test]
fn memory_formation_and_impure_parallel_refusal() {
    for method in ["physical_memory", "available_memory"] {
        for (source, expected) in [
            (format!("fn main() {{ x := os.{method}() }}"), "std.os"),
            (format!("import std.os\nfn main() {{ x := os.{method}(1) }}"), "takes no arguments"),
            (format!("import std.os\nfn f() -> i64 = os.{method}()\nfn main() {{}}"), "type mismatch"),
            (format!("import std.os\nfn f() -> Result<i32, Error> = os.{method}()\nfn main() {{}}"), "type mismatch"),
            (format!("import std.os\nfn main() {{ xs := [1,2]; ys := xs.par_map(fn x {{ observed := os.{method}() else {{ return x }}; observed }}).sum() }}"), "requires a Pure function"),
        ] {
            let checked = diff_check_multi("memory-invalid", &[("main.align", &source)], "main.align");
            assert!(checked.whole_errors && checked.per_unit_errors, "accepted: {source}");
            for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
                assert!(diagnostics.contains(expected), "expected {expected}: {diagnostics}");
            }
        }
        for (declaration, call) in [("observe()", "observe()"), ("observe<T>(marker: T)", "observe(x)")] {
            let helper = format!("module helper\nimport std.os\npub fn {declaration} -> Result<i64, Error> = os.{method}()");
            let main = format!("import helper\nfn main() {{ xs := [1,2]; ys := xs.par_map(fn x {{ observed := helper.{call} else {{ return x }}; observed }}).sum() }}");
            let checked = diff_check_multi("memory-import-effect", &[("helper.align", &helper), ("main.align", &main)], "main.align");
            assert!(checked.whole_errors && checked.per_unit_errors, "imported impure call accepted");
            for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
                assert!(diagnostics.contains("requires a Pure function"), "{diagnostics}");
            }
        }
    }
}
