//! Real credential observations and ordinary Copy record transport.
mod common;
use common::*;

#[test]
fn identity_observation_and_transport() {
    let helper = r#"module helper
import std.os
pub fn observe<T>(marker: T) -> Result<os.identity_info, Error> = arena { os.identity() }
"#;
    let main = r#"import std.os
import helper
fn main() -> Result<(), Error> {
  first := helper.observe(1)?
  mut copied := first
  copied = os.identity()?
  optional := Some(copied)
  last := optional else { return Err(Error.Invalid) }
  print(first.real_uid)
  print(last.real_gid)
  constructed := os.identity_info { real_uid: 123, real_gid: 456 }
  print(constructed.real_uid)
  return Ok(())
}
"#;
    let files = &[("helper.align",helper),("main.align",main)];
    let checked = diff_check_multi("os-identity",files,"main.align");
    assert!(!checked.whole_errors && !checked.per_unit_errors,"whole:{}\nunit:{}",checked.whole_diags,checked.per_unit_diags);
    if backend_available() {
        let expected = format!("{}\n{}\n123\n",unsafe { libc::getuid() },unsafe { libc::getgid() });
        for unit in [false,true] {
            let out = if unit { build_per_unit_multi("identity-unit",files,"main.align").link_and_run() }
                else { build_and_run_multi("identity-whole",files,"main.align") };
            assert_eq!(out.status.code(),Some(0),"{}",String::from_utf8_lossy(&out.stderr));
            assert_eq!(out.stdout,expected.as_bytes());
        }
    }
}

#[test]
fn identity_formation_and_effect() {
    for source in [
        "fn main() { x := os.identity() }",
        "import std.os\nfn main() { x := os.identity(1) }",
        "import std.os\nfn take(x: identity_info) {}\nfn main() {}",
        "import std.os\nfn main() { x := os.identity_info { real_uid: true, real_gid: 1 } }",
        "import std.os\nfn main() { xs := [1,2]; ys := xs.par_map(|x| { id := os.identity() else { return x }; x }) }",
    ] {
        let checked=diff_check_multi("identity-invalid",&[("main.align",source)],"main.align");
        assert!(checked.whole_errors && checked.per_unit_errors,"accepted: {source}");
    }
}
