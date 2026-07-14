//! `alignc` CLI (`docs/impl/01-pipeline.md`).
//!
//! Subcommands:
//!   alignc check     <file>   lexer -> parser -> sema. Print diagnostics
//!   alignc emit-interface <file>  Print each unit's interface summary + interface/impl hashes (M15)
//!   alignc emit-mir  <file>   Print MIR as text
//!   alignc emit-llvm <file>   Print LLVM IR as text (--stage raw|optimized; default raw)
//!   alignc emit-obj  <file>   Write an object file (no link, no `main` required)
//!   alignc explain-opt <file> Report the -O2 optimizer's data-path decisions (--verbose)
//!   alignc build     <file>   Build an executable (<stem> in cwd)
//!   alignc run       <file>   Build, run, and return its exit code
//!   alignc size      <file>   Build then report the executable's size breakdown
//!
//! A `--profile dev|release|fast|small|tiny` flag selects the optimization/size trade-off for the
//! build-producing subcommands (`build`/`run`/`emit-obj`/`size`); default `release`.
//!
//! A repeatable `--export <name>` flag (`emit-obj`/`emit-llvm` only) names an entry-file top-level
//! function that keeps external linkage instead of the default whole-program `internal` (M13 Slice
//! 1 internalized every program function) — the explicit export-roots mechanism restoring a linkable
//! C-ABI surface for a no-`main` library/benchmark object (`docs/impl/07-roadmap.md` M13 Codex-audit
//! item 1).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use align_driver::{
    build_interface_summaries, check, emit_llvm_ir, emit_object_file, format_diagnostics,
    link_executable, lower_to_mir, unknown_exports, BuildTarget, Profile,
};
use align_span::SourceMap;

mod size;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    // Pull the `--target-cpu` flag out before positional parsing (so it may sit anywhere up to the
    // program's own args, and `run` does not forward it to the built program).
    let (target, args) = parse_target(&raw);
    // Pull `--profile <name>` next (also anywhere before the program's own args). A bad value is a
    // hard error here, not a silent fallback.
    let (profile, args) = match parse_profile(&args) {
        Ok(v) => v,
        Err(bad) => {
            eprintln!("alignc: unknown --profile '{bad}' (expected one of: {})", Profile::NAMES);
            return ExitCode::FAILURE;
        }
    };
    // Pull every `--export <name>` out next, still before positional parsing — otherwise
    // `emit-obj kernels.align --export foo` would leave `foo` sitting where the output-object
    // positional argument is read from. A bare `--export` with no following value is a hard error.
    let (exports, args) = match parse_exports(&args) {
        Ok(v) => v,
        Err(()) => {
            eprintln!("alignc: --export requires a value (e.g. `--export foo`)");
            return ExitCode::FAILURE;
        }
    };
    // Pull the boolean `--rt-lto` flag (M14 Slice 2): opt-in in-process link of the fast-path string
    // primitives' bitcode into the program module before the one opt run. Orthogonal to `--profile`
    // (Nothing-hidden: the mechanism is named, not folded into `fast`).
    let (rt_lto, args) = parse_rt_lto(&args);
    let cmd = args.get(1).map(String::as_str);
    let path = args.get(2);

    // `--rt-lto` only means something where codegen runs the optimizer over a real build/lens.
    if rt_lto {
        if !matches!(
            cmd,
            Some("build") | Some("run") | Some("emit-obj") | Some("size") | Some("emit-llvm")
        ) {
            eprintln!(
                "alignc: --rt-lto is only valid for `build`/`run`/`emit-obj`/`size`/`emit-llvm` (got `{}`)",
                cmd.unwrap_or("<none>")
            );
            return ExitCode::FAILURE;
        }
        // `dev` is O0 (nothing inlines, so LTO buys nothing); `small`/`tiny` run the `optsize`/
        // `minsize` sweep, which conflicts with fast-path inlining. Reject rather than silently no-op.
        if matches!(profile, Profile::Dev | Profile::Small | Profile::Tiny) {
            eprintln!(
                "alignc: --rt-lto is incompatible with the `{}` profile (it needs an inlining \
                 pipeline; use `release` or `fast`)",
                profile.name()
            );
            return ExitCode::FAILURE;
        }
    }

    // `--export` only means something where codegen produces a standalone object/IR with linker-
    // visible symbols (`emit-obj`/`emit-llvm`); anywhere else a nonempty export set would either be
    // silently ignored or silently change linkage no one asked for — neither is acceptable
    // (Nothing hidden), so reject it outright instead.
    if !exports.is_empty() && !matches!(cmd, Some("emit-obj") | Some("emit-llvm")) {
        eprintln!(
            "alignc: --export is only valid for `emit-obj`/`emit-llvm` (got `{}`)",
            cmd.unwrap_or("<none>")
        );
        return ExitCode::FAILURE;
    }

    match (cmd, path) {
        (Some("check"), Some(p)) => run_check(p),
        (Some("emit-interface"), Some(p)) => run_emit_interface(p),
        (Some("emit-mir"), Some(p)) => run_emit_mir(p),
        (Some("emit-llvm"), Some(p)) => run_emit_llvm(p, args.get(3..).unwrap_or(&[]), target, &exports, rt_lto),
        // `emit-obj <file> [out.o]` — codegen to an object file, no linking and no `main` required
        // (a library / benchmark kernel). Default output is `<stem>.o`.
        (Some("emit-obj"), Some(p)) => run_emit_obj(p, args.get(3).map(String::as_str), target, profile, &exports, rt_lto),
        // `size <file>` — build with the profile, then report the executable's size breakdown.
        (Some("size"), Some(p)) => size::run_size(p, target, profile, rt_lto),
        // `explain-opt <file> [--verbose]` — report what the `-O2` middle-end did to the data path
        // (vectorized / not, with the reason), translated into the compiler's diagnostic voice.
        (Some("explain-opt"), Some(p)) => {
            let verbose = args.get(3..).unwrap_or(&[]).iter().any(|a| a == "--verbose" || a == "-v");
            align_driver::explain::run_explain_opt(p, verbose, target)
        }
        // `fmt <file> [--write]` — format source; prints to stdout, or rewrites in place with --write.
        (Some("fmt"), Some(p)) => run_fmt(p, &args[3..]),
        (Some("build"), Some(p)) => run_build(p, target, profile, rt_lto),
        // `run` forwards any trailing arguments to the built program (its `main(args)`).
        (Some("run"), Some(p)) => run_run(p, &args[3..], target, profile, rt_lto),
        _ => {
            usage();
            ExitCode::FAILURE
        }
    }
}

/// Pull every `--export <name>` / `--export=<name>` out of `args` (repeatable — each occurrence
/// adds one name; no comma-separated lists), returning the collected export roots in order and the
/// remaining (positional) arguments. A bare `--export` with no following value is `Err(())` — a hard
/// error, never a silently-ignored flag or a guessed name.
fn parse_exports(args: &[String]) -> Result<(Vec<String>, Vec<String>), ()> {
    let mut exports = Vec::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(v) = a.strip_prefix("--export=") {
            exports.push(v.to_string());
        } else if a == "--export" {
            match args.get(i + 1) {
                Some(v) => {
                    exports.push(v.clone());
                    i += 1;
                }
                None => return Err(()),
            }
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    Ok((exports, rest))
}

/// Pull the boolean `--rt-lto` flag out of `args` (M14 Slice 2), returning whether it was present and
/// the remaining arguments. A valueless flag — repeated occurrences are idempotent.
fn parse_rt_lto(args: &[String]) -> (bool, Vec<String>) {
    let mut rt_lto = false;
    let mut rest = Vec::new();
    for a in args {
        if a == "--rt-lto" {
            rt_lto = true;
        } else {
            rest.push(a.clone());
        }
    }
    (rt_lto, rest)
}

/// Validate `exports` against the lowered `mir`: every name must match a `Function::name` in the
/// program, or this is a hard failure listing every unknown name (never a silent no-op — a typo'd
/// export name must not compile a wrong object with no diagnostic). Returns the failure `ExitCode`
/// on rejection, `None` when every export resolves.
fn check_exports(mir: &align_mir::Program, exports: &[String], path: &str) -> Option<ExitCode> {
    let unknown = unknown_exports(mir, exports);
    if unknown.is_empty() {
        return None;
    }
    eprintln!("alignc: unknown export(s): {} (not defined in {path})", unknown.join(", "));
    Some(ExitCode::FAILURE)
}

/// Pull `--target-cpu <baseline|native>` (or `--target-cpu=…`) out of `args`, returning the chosen
/// target and the remaining (positional) arguments. Default = the portable `Baseline`.
fn parse_target(args: &[String]) -> (BuildTarget, Vec<String>) {
    // `baseline` / `native` are keywords; anything else is passed to LLVM as a CPU name
    // (`x86-64-v3`, `znver3`, …) — the portable-performance tier for a fleet you control.
    let value = |v: &str| match v {
        "native" => BuildTarget::Native,
        "baseline" => BuildTarget::Baseline,
        other => BuildTarget::Cpu(other.to_string()),
    };
    let mut target = BuildTarget::Baseline;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(v) = a.strip_prefix("--target-cpu=") {
            target = value(v);
        } else if a == "--target-cpu" {
            if let Some(v) = args.get(i + 1) {
                target = value(v);
                i += 1;
            } else {
                eprintln!("alignc: missing value for --target-cpu (expected `baseline` or `native`); using baseline");
            }
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    (target, rest)
}

/// Pull `--profile <name>` (or `--profile=…`) out of `args`, returning the chosen profile and the
/// remaining (positional) arguments. Default = `release` (today's behavior — a build with no flag
/// runs `default<O2>`, so there is no behavior change without the flag). Exact names only; any other
/// value is `Err(value)` so the caller emits a diagnostic rather than guessing. A bare `--profile`
/// with no following value reads as the empty string, which is rejected like any unknown value.
fn parse_profile(args: &[String]) -> Result<(Profile, Vec<String>), String> {
    let mut profile = Profile::default();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let value = if let Some(v) = a.strip_prefix("--profile=") {
            Some(v.to_string())
        } else if a == "--profile" {
            let v = args.get(i + 1).map(String::as_str).unwrap_or("").to_string();
            i += 1;
            Some(v)
        } else {
            rest.push(a.clone());
            None
        };
        if let Some(v) = value {
            profile = Profile::parse(&v).ok_or(v)?;
        }
        i += 1;
    }
    Ok((profile, rest))
}

fn usage() {
    eprintln!(
        "usage: alignc <command> <file.align> [--target-cpu baseline|native]\n\
         \n\
         commands:\n  \
           check      check through lexer/parser/sema\n  \
           emit-interface  print each unit's interface summary + interface/impl hashes\n  \
           emit-mir   print MIR as text\n  \
           emit-llvm  print LLVM IR as text (--stage raw|optimized; default raw)\n  \
           emit-obj   write an object file (<file> [out.o]; no link, no `main` needed)\n  \
           explain-opt report what the -O2 optimizer did to the data path (--verbose for detail)\n  \
           fmt        format source (prints to stdout; --write rewrites in place)\n  \
           build      build an executable\n  \
           run        build and run (returns the exit code)\n  \
           size       build then report the executable's size breakdown\n  \
         \n\
         --target-cpu  baseline (default; portable per-arch floor), native (this host's CPU),\n  \
                       or an LLVM CPU name like x86-64-v3 (a portable fast tier for a known fleet)\n  \
         --profile     dev (O0), release (O2, default), fast (O3), small (Os), tiny (Oz)\n  \
         --export      (emit-obj/emit-llvm only; repeatable) keep an entry-file top-level function\n  \
                       name's linkage external instead of the default internal, so a no-`main`\n  \
                       library/benchmark object exposes it to the linker\n  \
         --rt-lto      (build/run/emit-obj/size/emit-llvm; release/fast only) link the fast-path\n  \
                       string primitives' bitcode into the program and inline it before the opt run"
    );
}

fn read(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("alignc: cannot read '{path}': {e}");
            None
        }
    }
}

/// check -> MIR. On error, print diagnostics and return `None`.
fn front_to_mir(path: &str) -> Option<align_mir::Program> {
    let src = read(path)?;
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, path, &src);
    if !checked.diags.is_empty() {
        eprint!("{}", format_diagnostics(&sm, &checked.diags));
    }
    if checked.diags.has_errors() {
        return None;
    }
    Some(lower_to_mir(&checked.hir))
}

/// `fmt <file> [--write]` — format the source. Without `--write`, print the formatted text to
/// stdout (a read-only default); with `--write`/`-w`, rewrite the file in place only if it changed.
/// If the source does not parse cleanly, it is left untouched (and `--write` is a no-op) — the
/// formatter never emits from a partial parse.
fn run_fmt(path: &str, flags: &[String]) -> ExitCode {
    let write = flags.iter().any(|f| f == "--write" || f == "-w");
    let Some(src) = read(path) else {
        return ExitCode::FAILURE;
    };
    let Some(formatted) = align_fmt::format_source(0, &src) else {
        eprintln!("alignc: cannot format '{path}' (it does not parse cleanly); left unchanged");
        return ExitCode::FAILURE;
    };
    if !write {
        print!("{formatted}");
    } else if formatted != src && let Err(e) = std::fs::write(path, &formatted) {
        eprintln!("alignc: cannot write '{path}': {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_check(path: &str) -> ExitCode {
    let Some(src) = read(path) else {
        return ExitCode::FAILURE;
    };
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, path, &src);
    if !checked.diags.is_empty() {
        eprint!("{}", format_diagnostics(&sm, &checked.diags));
    }
    if checked.diags.has_errors() {
        ExitCode::FAILURE
    } else {
        println!("ok: checked {} function(s)", checked.hir.fns.len());
        ExitCode::SUCCESS
    }
}

/// `alignc emit-interface <file>` (M15 S1a, dev verb): print each unit's interface summary — its
/// interface / impl hashes, exported signatures with effect bits, exported type defs, consts, and
/// capability set. A human-readable rendering of [`build_interface_summaries`]; the byte artifact is
/// the crate's `serialize`. Deterministic (units and exported items are name-sorted at build time).
fn run_emit_interface(path: &str) -> ExitCode {
    let Some(src) = read(path) else {
        return ExitCode::FAILURE;
    };
    let mut sm = SourceMap::new();
    let (summaries, diags) = build_interface_summaries(&mut sm, path, &src);
    if !diags.is_empty() {
        eprint!("{}", format_diagnostics(&sm, &diags));
    }
    if diags.has_errors() {
        return ExitCode::FAILURE;
    }
    for s in &summaries {
        println!("unit {}", s.unit);
        println!("  interface_hash {}", s.interface_hash.to_hex());
        println!("  impl_hash      {}", s.impl_hash.to_hex());
        if !s.capabilities.is_empty() {
            println!("  capabilities   {}", s.capabilities.join(", "));
        }
        for f in &s.fns {
            let tps = if f.type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", f.type_params.iter().map(|t| t.name.clone()).collect::<Vec<_>>().join(", "))
            };
            println!("  pub fn {}{} [{:?}]", f.name, tps, f.effect);
        }
        for st in &s.structs {
            println!("  pub struct {} ({} field(s))", st.name, st.fields.len());
        }
        for e in &s.enums {
            println!("  pub enum {} ({} variant(s))", e.name, e.variants.len());
        }
        for c in &s.consts {
            println!("  pub const {}", c.name);
        }
    }
    ExitCode::SUCCESS
}

fn run_emit_mir(path: &str) -> ExitCode {
    match front_to_mir(path) {
        Some(mir) => {
            print!("{}", align_mir::print::program_to_string(&mir));
            ExitCode::SUCCESS
        }
        None => ExitCode::FAILURE,
    }
}

fn run_emit_llvm(path: &str, rest: &[String], target: BuildTarget, exports: &[String], rt_lto: bool) -> ExitCode {
    // `--stage raw|optimized` picks the lens (default `raw` = today's semantics, the pre-opt IR
    // codegen emitted). `optimized` runs the `-O2` pipeline first (what LLVM did: inlined, fused,
    // vectorized). Any other value is a hard argument error, not a panic.
    let optimized = match parse_stage(rest) {
        Ok(v) => v,
        Err(bad) => {
            eprintln!("alignc: unknown --stage '{bad}' (expected `raw` or `optimized`)");
            return ExitCode::FAILURE;
        }
    };
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    if let Some(code) = check_exports(&mir, exports, path) {
        return code;
    }
    match emit_llvm_ir(&mir, target, optimized, exports, rt_lto) {
        Ok(ir) => {
            print!("{ir}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("alignc: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Parse `--stage raw|optimized` (or `--stage=…`) out of the trailing `emit-llvm` args. Returns
/// `Ok(true)` for `optimized`, `Ok(false)` for `raw` or when absent (the default lens), or
/// `Err(bad_value)` for any other `--stage` value. A missing value after a bare `--stage` reads as
/// the empty string, which is rejected like any other unknown value.
fn parse_stage(rest: &[String]) -> Result<bool, String> {
    let mut i = 0;
    let mut optimized = false;
    while i < rest.len() {
        let a = &rest[i];
        let value = if let Some(v) = a.strip_prefix("--stage=") {
            Some(v.to_string())
        } else if a == "--stage" {
            let v = rest.get(i + 1).map(String::as_str).unwrap_or("");
            i += 1;
            Some(v.to_string())
        } else {
            None
        };
        if let Some(v) = value {
            optimized = match v.as_str() {
                "raw" => false,
                "optimized" => true,
                other => return Err(other.to_string()),
            };
        }
        i += 1;
    }
    Ok(optimized)
}

fn run_emit_obj(path: &str, out: Option<&str>, target: BuildTarget, profile: Profile, exports: &[String], rt_lto: bool) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    if let Some(code) = check_exports(&mir, exports, path) {
        return code;
    }
    let obj = PathBuf::from(out.map(String::from).unwrap_or_else(|| format!("{}.o", stem(path))));
    match emit_object_file(&mir, &obj, target, profile, exports, rt_lto) {
        Ok(()) => {
            println!("alignc: wrote object: {}", obj.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("alignc: codegen failed: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Use the source file name (without extension) as the output name.
fn stem(path: &str) -> String {
    PathBuf::from(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "a".to_string())
}

static ARTIFACT_NONCE: AtomicU64 = AtomicU64::new(0);

/// A process-private staging directory. `create_dir` is the atomic claim: a stale or racing path is
/// skipped, and Drop removes only the directory this invocation successfully created.
struct ArtifactStage {
    dir: PathBuf,
}

impl ArtifactStage {
    fn in_dir(parent: &Path, label: &str) -> std::io::Result<Self> {
        let parent = std::fs::canonicalize(parent)?;
        for _ in 0..1024 {
            let nonce = ARTIFACT_NONCE.fetch_add(1, Ordering::Relaxed);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = parent.join(format!(".{label}-{}-{stamp}-{nonce}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Self { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique artifact staging directory",
        ))
    }

    fn temp(label: &str) -> std::io::Result<Self> {
        Self::in_dir(&std::env::temp_dir(), label)
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for ArtifactStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Turn MIR into an object and link it into an executable. Returns the `exe` path. `profile` selects
/// both the codegen pipeline and the link-time strip choice (one mechanism, `Profile`).
fn build_to(_path: &str, mir: &align_mir::Program, exe: &Path, target: BuildTarget, profile: Profile, rt_lto: bool) -> Result<(), ExitCode> {
    let object_stage = ArtifactStage::temp("align-object").map_err(|e| {
        eprintln!("alignc: cannot create object staging directory: {e}");
        ExitCode::FAILURE
    })?;
    let obj = object_stage.path().join("program.o");
    // `build`/`run`/`size` never take `--export` — they always produce a linked executable whose
    // only linker-visible symbol is `main`, so no export roots apply here.
    if let Err(e) = emit_object_file(mir, &obj, target, profile, &[], rt_lto) {
        eprintln!("alignc: codegen failed: {e}");
        return Err(ExitCode::FAILURE);
    }
    let parent = exe.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let output_stage = ArtifactStage::in_dir(parent, "align-publish").map_err(|e| {
        eprintln!("alignc: cannot create executable staging directory: {e}");
        ExitCode::FAILURE
    })?;
    let staged_exe = output_stage.path().join(exe.file_name().unwrap_or_else(|| std::ffi::OsStr::new("program")));
    if let Err(e) = link_executable(&obj, &staged_exe, &mir.link_libs, profile) {
        eprintln!("alignc: {e}");
        return Err(ExitCode::FAILURE);
    }
    if let Err(e) = std::fs::rename(&staged_exe, exe) {
        eprintln!("alignc: cannot publish executable {}: {e}", exe.display());
        return Err(ExitCode::FAILURE);
    }
    Ok(())
}

fn run_build(path: &str, target: BuildTarget, profile: Profile, rt_lto: bool) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let exe = PathBuf::from(stem(path));
    match build_to(path, &mir, &exe, target, profile, rt_lto) {
        Ok(()) => {
            println!("alignc: built executable: {}", exe.display());
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn run_run(path: &str, prog_args: &[String], target: BuildTarget, profile: Profile, rt_lto: bool) -> ExitCode {
    let Some(mir) = front_to_mir(path) else {
        return ExitCode::FAILURE;
    };
    let stage = match ArtifactStage::temp("align-run") {
        Ok(stage) => stage,
        Err(e) => {
            eprintln!("alignc: cannot create run staging directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let exe = stage.path().join("program");
    if let Err(code) = build_to(path, &mir, &exe, target, profile, rt_lto) {
        return code;
    }
    // Forward trailing args so they reach the program's `main(args: array<str>)` (argv[0] is the
    // executable, then `prog_args`).
    match std::process::Command::new(&exe).args(prog_args).status() {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(code as u8),
            None => {
                eprintln!("alignc: process terminated by a signal");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("alignc: cannot run: {e}");
            ExitCode::FAILURE
        }
    }
}
