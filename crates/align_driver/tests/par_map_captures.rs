//! Sequential fallback captures retain original HIR identities across control paths.
#![cfg(unix)]
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use common::*;

#[test]
fn par_map_capture_identity() {
    owned_fixture::run("par_map_capture_identity", |_| {
        if !backend_available() {
            return;
        }
        let mut source = String::from(
            "module model\npub Inner {value:i64}\npub Outer {inner:Inner}\npub fn identity<T>(x:T)->T = x\npub fn branch(which:i64)->i64 {\nxs:=[Outer {inner:Inner {value:1}},Outer {inner:Inner {value:2}}]\n",
        );
        for arm in 0..16 {
            source.push_str(&format!("k{arm}:={}\n", arm * 10));
        }
        source.push_str("result:=");
        for arm in 0..16 {
            if arm != 15 {
                source.push_str(&format!("if which=={arm} "));
            }
            source.push_str(&format!(
                "{{xs.inner.value.par_map(fn x:i64 {{x+k{arm}}})}}"
            ));
            if arm != 15 {
                source.push_str(" else ");
            }
        }
        source.push_str("\nreturn result.sum()\n}\n");
        source.push_str(MODEL);
        let mut main = String::from("module main\nimport model\nfn main(){\n");
        let mut expected = String::new();
        for arm in 0..16 {
            main.push_str(&format!("print(model.branch({arm}))\n"));
            expected.push_str(&format!("{}\n", arm * 20 + 3));
        }
        for (which, sum) in [(0, 25), (1, 47), (2, 0)] {
            main.push_str(&format!("print(model.staged({which}).sum())\n"));
            expected.push_str(&format!("{sum}\n"));
        }
        main.push_str(
            "print(model.columns(true))\nprint(model.columns(false))\nprint(model.control())\n}\n",
        );
        expected.push_str("12\n43\ntrue\n");
        let files = [
            ("main.align", main.as_str()),
            ("model.align", source.as_str()),
        ];
        assert_output(
            build_and_run_multi("sequential-captures", &files, "main.align"),
            &expected,
        );
        let project = Proj::new("sequential-capture-replay", &files, "main.align");
        for round in 0..2 {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
                .args(["run", "main.align", "--cache-stats", "-j", "1"])
                .current_dir(&project.dir)
                .env("ALIGNC_CACHE", project.cache_root())
                // The whole-program run above also owns measurement-off variants; replay
                // requires the ordinary cache-enabled compiler settings in these children.
                .env_remove("ALIGN_NEEDLE_HOIST")
                .env_remove("ALIGN_BUFFER_DONATE")
                .output()
                .unwrap();
            let cache = if round == 0 {
                "2 frontend: 0 hit, 2 miss"
            } else {
                "2 frontend: 2 hit, 0 miss"
            };
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(cache),
                "{output:?}"
            );
            assert_output(output, &expected);
        }
    });
}

fn assert_output(output: std::process::Output, expected: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

const MODEL: &str = r#"
fn make()->array<Outer> = [Outer {inner:Inner {value:1}},Outer {inner:Inner {value:2}}].to_array()
pub fn staged(which:i64)->array<i32> {
  first:=1
  second:=2
  third:=99
  a:=10
  b:=20
  c:=30
  rows:=make()
  return if which==0 {
    rows.inner.value.map(fn x:i64 {identity(x+first)}).where(fn x:i64 {x>first}).par_map(fn x:i64 {(x+a) as i32})
  } else if which==1 {
    rows.inner.value.map(fn x:i64 {identity(x+second)}).where(fn x:i64 {x>second}).par_map(fn x:i64 {(x+b) as i32})
  } else {
    rows.inner.value.map(fn x:i64 {identity(x+third)}).where(fn x:i64 {x>999}).par_map(fn x:i64 {(x+c) as i32})
  }
}
Text {active:bool,text:str}
pub fn columns(which:bool)->i64 = arena {
  rows:=[Text {active:true,text:"ab"},Text {active:true,text:"b"},Text {active:false,text:"abc"}].to_soa()
  a:="a"
  b:="b"
  first:=10
  second:=20
  result:=if which {
    rows.where(.active).text.where(fn text:str {text.contains(a)}).par_map(fn text:str {text.len()+first})
  } else {
    rows.where(.active).text.where(fn text:str {text.contains(b)}).par_map(fn text:str {text.len()+second})
  }
  result.sum()
}
fn early(which:bool)->i64 {
  rows:=if which {return 7} else {make()}
  k:=5
  return rows.par_map(fn row:Outer {row.inner.value+k}).sum()
}
fn numbers()->array<i64> = [1,2].to_array()
pub fn control()->bool {
  k:=10
  rows:=make()
  direct:=rows.par_map(fn row:Outer {identity(row.inner.value+k) as f64})
  parallel:=[1,2].par_map(fn x:i64 {x+k})
  temporary:=numbers().map(fn x:i64 {Outer {inner:Inner {value:x}}}).par_map(fn row:Outer {(row.inner.value+k) as f64})
  mut replaced:=staged(0)
  replaced=staged(1)
  selected:=match 1 {1=>staged(0),_=>staged(1)}
  joined:=loop {break staged(1)}
  _:=staged(0)
  return temporary.sum()==23.0 && direct.sum()==23.0 && parallel.sum()==23 && replaced.sum()==47 && selected.sum()==25 && joined.sum()==47 && early(true)==7 && early(false)==13
}
"#;
