//! M0 端から端まで: `.align` ソースが lexer → parser → sema → MIR を通り、
//! 期待する MIR が出ることを検証する (`docs/impl/07-roadmap.md` M0)。
//! LLVM 段は未結線のため、現状の縦切りの終点 = MIR で検証する。

use align_driver::{check, lower_to_mir};
use align_span::SourceMap;

const M0: &str = "fn main() -> i32 {\n  x := 1\n  return x\n}\n";

#[test]
fn m0_pipeline_to_mir() {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m0.align", M0);
    assert!(
        !checked.diags.has_errors(),
        "M0 プログラムでエラーが出た"
    );

    let mir = lower_to_mir(&checked.hir);
    let text = align_mir::print::program_to_string(&mir);

    // x := 1 が i32 定数として束縛され、return がその値を返す。
    assert!(text.contains("fn main() -> i32"), "got:\n{text}");
    assert!(text.contains("1_i32"), "i32 定数が無い:\n{text}");
    assert!(text.contains("return %"), "値の return が無い:\n{text}");
}

#[test]
fn m0_compiles_and_runs_with_exit_code() {
    // M0 完了条件: alignc run 相当が実行ファイルを出し、終了コードを返す。
    // LLVM/cc が必要なため、無い環境ではスキップする。
    if !align_driver::backend_available() {
        eprintln!("skip: LLVM バックエンド未結線");
        return;
    }
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m0.align", M0);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);

    let dir = std::env::temp_dir();
    let obj = dir.join("align-test-m0.o");
    let exe = dir.join("align-test-m0");
    align_driver::emit_object_file(&mir, &obj).expect("codegen");
    align_driver::link_executable(&obj, &exe).expect("link");

    let status = std::process::Command::new(&exe).status().expect("run");
    assert_eq!(status.code(), Some(1), "main は x:=1 を返すので終了コード 1");
}

#[test]
fn type_mismatch_is_reported() {
    // () を返す関数で整数を返そうとするとエラー。
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "bad.align", "fn f() {\n  return 1\n}\n");
    assert!(checked.diags.has_errors());
}
