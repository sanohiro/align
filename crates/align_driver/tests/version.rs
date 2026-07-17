//! The package-manager contract: version discovery must not require a source file or LLVM work.

#[test]
fn version_flag_reports_workspace_package_version() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .arg("--version")
        .output()
        .expect("run alignc --version");

    assert!(output.status.success(), "--version must succeed");
    assert_eq!(
        String::from_utf8(output.stdout).expect("version output is UTF-8"),
        format!("alignc {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty(), "--version must not write diagnostics");
}
