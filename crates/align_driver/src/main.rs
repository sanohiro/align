//! `alignc` CLI (`docs/impl/01-pipeline.md`).
//!
//! Subcommands:
//!   alignc check     <file>   lexer -> parser -> sema. Print diagnostics
//!   alignc check-per-unit <file>  Check each unit against its imports' interface summaries (M15 S1b)
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
    build_interface_summaries, build_per_unit, check, emit_llvm_ir, emit_object_cached,
    format_diagnostics, link_objects, unknown_exports, BuildTarget, CacheContext, PerUnitWalk,
    Profile,
};
use align_span::SourceMap;

mod size;

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    // Package-manager smoke tests and bug reports need a cheap, source-free way to identify the
    // compiler. Keep this before flag parsing: `--version` is a complete invocation, not a build
    // flag, and must not be mistaken for a subcommand.
    if raw.len() == 2 && matches!(raw[1].as_str(), "--version" | "-V") {
        println!("alignc {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Pull the instrument-PGO flags FIRST (`--pgo-instrument` / `--pgo-use <file.profdata>`, S1):
    // mutually exclusive; a bare `--pgo-use` is a hard error. It must run before the other flag
    // strippers so `--pgo-use`'s likely-flag guard sees a following flag (`--thin-lto`, `--profile`,
    // …) still present — otherwise that flag would already be removed and the guard would consume the
    // verb as the profile value. Serial, cache-bypassed, release/fast only.
    let (pgo, args) = match parse_pgo(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("alignc: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Pull the `--target-cpu` flag out before positional parsing (so it may sit anywhere up to the
    // program's own args, and `run` does not forward it to the built program).
    let (target, args) = parse_target(&args);
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
    // Pull the boolean `--thin-lto` flag (ThinLTO S1): opt-in cross-unit optimization. Serial, no
    // cache, release/fast only. Orthogonal to (and composable with) `--rt-lto`.
    let (thin_lto, args) = parse_thin_lto(&args);
    // Pull `--cache-stats` (S3b, build/run/size only) and the `-j`/`--jobs` codegen-parallelism flag.
    let (cache_stats, args) = parse_cache_stats(&args);
    let (jobs_flag, args) = match parse_jobs(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("alignc: {e}");
            return ExitCode::FAILURE;
        }
    };
    let cmd = args.get(1).map(String::as_str);
    let path = args.get(2);

    // `--cache-stats` / `-j` only mean something on the build-producing per-unit path.
    let build_verb = matches!(cmd, Some("build") | Some("run") | Some("size"));
    if cache_stats && !build_verb {
        eprintln!("alignc: --cache-stats is only valid for `build`/`run`/`size` (got `{}`)", cmd.unwrap_or("<none>"));
        return ExitCode::FAILURE;
    }
    if jobs_flag.is_some() && !build_verb {
        eprintln!("alignc: -j/--jobs is only valid for `build`/`run`/`size` (got `{}`)", cmd.unwrap_or("<none>"));
        return ExitCode::FAILURE;
    }

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

    // `--thin-lto` links N per-unit objects with cross-unit optimization, so it only means something
    // on the build-producing verbs that link a whole program (`build`/`run`/`size`). `emit-obj` /
    // `emit-llvm` are per-unit-in-isolation by settlement (the honest zero-cross-unit-opt lens), so
    // ThinLTO is rejected there rather than silently ignored.
    if thin_lto {
        if !matches!(cmd, Some("build") | Some("run") | Some("size")) {
            eprintln!(
                "alignc: --thin-lto is only valid for `build`/`run`/`size` (got `{}`)",
                cmd.unwrap_or("<none>")
            );
            return ExitCode::FAILURE;
        }
        // Same profile constraint as `--rt-lto`: `dev` is O0 (nothing inlines), and `small`/`tiny`
        // run the size sweep — ThinLTO needs an inlining pipeline. Reject rather than silently no-op.
        if matches!(profile, Profile::Dev | Profile::Small | Profile::Tiny) {
            eprintln!(
                "alignc: --thin-lto is incompatible with the `{}` profile (it needs an inlining \
                 pipeline; use `release` or `fast`)",
                profile.name()
            );
            return ExitCode::FAILURE;
        }
    }

    // Instrument-PGO (`--pgo-instrument` / `--pgo-use`) is legal only on the whole-program build verbs
    // (`build`/`run`/`size`) and the inlining profiles (`release`/`fast`) — the same discipline as
    // `--thin-lto` — and is REJECTED loudly combined with `--thin-lto` in v1 (correct ThinLTO+PGO
    // needs PGOOptions threaded through all three ThinLTO phases + profile-aware import; its own later
    // slice). `--rt-lto` composes freely. Mutual exclusion + a bare `--pgo-use` were already caught in
    // `parse_pgo`.
    if pgo.is_on() {
        if !matches!(cmd, Some("build") | Some("run") | Some("size")) {
            eprintln!(
                "alignc: --pgo-instrument/--pgo-use is only valid for `build`/`run`/`size` (got `{}`)",
                cmd.unwrap_or("<none>")
            );
            return ExitCode::FAILURE;
        }
        if matches!(profile, Profile::Dev | Profile::Small | Profile::Tiny) {
            eprintln!(
                "alignc: --pgo-instrument/--pgo-use is incompatible with the `{}` profile (it needs an \
                 inlining pipeline; use `release` or `fast`)",
                profile.name()
            );
            return ExitCode::FAILURE;
        }
        if thin_lto {
            eprintln!(
                "alignc: --pgo-instrument/--pgo-use cannot be combined with --thin-lto in v1 \
                 (ThinLTO+PGO is a separate later slice)"
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

    // Resolve the codegen worker count once (build verbs only); a bad `ALIGNC_JOBS` fails here.
    let jobs = if build_verb {
        match resolve_jobs(jobs_flag) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("alignc: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        1
    };

    match (cmd, path) {
        (Some("check"), Some(p)) => run_check(p),
        (Some("check-per-unit"), Some(p)) => run_check_per_unit(p),
        (Some("emit-interface"), Some(p)) => run_emit_interface(p),
        (Some("emit-mir"), Some(p)) => run_emit_mir(p),
        (Some("emit-llvm"), Some(p)) => run_emit_llvm(p, args.get(3..).unwrap_or(&[]), target, &exports, rt_lto),
        // `emit-obj <file> [out.o]` — codegen to an object file, no linking and no `main` required
        // (a library / benchmark kernel). Default output is `<stem>.o`.
        (Some("emit-obj"), Some(p)) => run_emit_obj(p, args.get(3).map(String::as_str), target, profile, &exports, rt_lto),
        // `size <file>` — build with the profile, then report the executable's size breakdown.
        (Some("size"), Some(p)) => size::run_size(p, target, profile, rt_lto, thin_lto, &pgo, jobs, cache_stats),
        // `cache clear` — remove the cache-owned subtrees under the resolved cache root (S3b).
        (Some("cache"), Some(sub)) if sub == "clear" => run_cache_clear(),
        (Some("cache"), other) => {
            eprintln!("alignc: unknown `cache` subcommand `{}` (expected: clear)", other.map(|s| s.as_str()).unwrap_or("<none>"));
            ExitCode::FAILURE
        }
        // `explain-opt <file> [--verbose]` — report what the `-O2` middle-end did to the data path
        // (vectorized / not, with the reason), translated into the compiler's diagnostic voice.
        (Some("explain-opt"), Some(p)) => {
            let verbose = args.get(3..).unwrap_or(&[]).iter().any(|a| a == "--verbose" || a == "-v");
            align_driver::explain::run_explain_opt(p, verbose, target)
        }
        // `fmt <file> [--write]` — format source; prints to stdout, or rewrites in place with --write.
        (Some("fmt"), Some(p)) => run_fmt(p, &args[3..]),
        (Some("build"), Some(p)) => run_build(p, target, profile, rt_lto, thin_lto, &pgo, jobs, cache_stats),
        // `run` forwards any trailing arguments to the built program (its `main(args)`).
        (Some("run"), Some(p)) => run_run(p, &args[3..], target, profile, rt_lto, thin_lto, &pgo, jobs, cache_stats),
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

/// Pull the boolean `--thin-lto` flag (ThinLTO S1) out of `args`, returning whether it was present
/// and the remaining arguments. Valueless; repeated occurrences are idempotent.
fn parse_thin_lto(args: &[String]) -> (bool, Vec<String>) {
    let mut thin_lto = false;
    let mut rest = Vec::new();
    for a in args {
        if a == "--thin-lto" {
            thin_lto = true;
        } else {
            rest.push(a.clone());
        }
    }
    (thin_lto, rest)
}

/// Pull the instrument-PGO flags (`--pgo-instrument` / `--pgo-use <path>` / `--pgo-use=<path>`, S1),
/// returning the resolved [`align_driver::PgoMode`] and the remaining args. Errors (all hard):
///   * a bare `--pgo-use` with no following value (never a guessed profile path);
///   * `--pgo-instrument` and `--pgo-use` together (mutually exclusive — an instrument build and a
///     use build are different artifacts).
fn parse_pgo(args: &[String]) -> Result<(align_driver::PgoMode, Vec<String>), String> {
    let mut instrument = false;
    let mut use_path: Option<String> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--pgo-instrument" {
            instrument = true;
        } else if let Some(v) = a.strip_prefix("--pgo-use=") {
            if v.is_empty() {
                return Err("--pgo-use requires a value (e.g. `--pgo-use app.profdata`)".to_string());
            }
            use_path = Some(v.to_string());
        } else if a == "--pgo-use" {
            match args.get(i + 1) {
                // Likely-flag guard: a value starting with `--` is another flag (or, transitively,
                // the verb), not a profile path — consuming it silently swallows the flag and, worse,
                // bypasses the mutual-exclusion check below (`--pgo-use --pgo-instrument` would set the
                // path to "--pgo-instrument" and leave `instrument` false). Reject it as a missing value.
                Some(v) if !v.starts_with("--") => {
                    use_path = Some(v.clone());
                    i += 1;
                }
                _ => {
                    return Err("--pgo-use requires a value (e.g. `--pgo-use app.profdata`)".to_string());
                }
            }
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    let mode = match (instrument, use_path) {
        (true, Some(_)) => {
            return Err("--pgo-instrument and --pgo-use are mutually exclusive".to_string());
        }
        (true, None) => align_driver::PgoMode::Instrument,
        (false, Some(p)) => align_driver::PgoMode::Use(std::path::PathBuf::from(p)),
        (false, None) => align_driver::PgoMode::Off,
    };
    Ok((mode, rest))
}

/// Pull the boolean `--cache-stats` flag (M15 S3b) out of `args`. Valueless, idempotent; with it, the
/// build/run/size verbs print a per-unit cache hit/miss report + a summary line (silent otherwise).
fn parse_cache_stats(args: &[String]) -> (bool, Vec<String>) {
    let mut stats = false;
    let mut rest = Vec::new();
    for a in args {
        if a == "--cache-stats" {
            stats = true;
        } else {
            rest.push(a.clone());
        }
    }
    (stats, rest)
}

/// Pull the `-j <N>` / `-j<N>` / `--jobs <N>` codegen-parallelism flag (M15 S3b). Returns the explicit
/// job count (if any) and the remaining args. A missing value or a non-`usize`/zero value is a hard
/// error (never a silent fallback). The flag wins over `ALIGNC_JOBS`; the default (neither set) is
/// [`std::thread::available_parallelism`].
fn parse_jobs(args: &[String]) -> Result<(Option<usize>, Vec<String>), String> {
    let mut jobs: Option<usize> = None;
    let mut rest = Vec::new();
    let mut i = 0;
    let parse_n = |s: &str| -> Result<usize, String> {
        match s.parse::<usize>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(format!("invalid job count '{s}' (expected a positive integer)")),
        }
    };
    while i < args.len() {
        let a = &args[i];
        if let Some(v) = a.strip_prefix("--jobs=").or_else(|| a.strip_prefix("-j")).filter(|v| !v.is_empty()) {
            jobs = Some(parse_n(v)?);
        } else if a == "-j" || a == "--jobs" {
            match args.get(i + 1) {
                Some(v) => {
                    jobs = Some(parse_n(v)?);
                    i += 1;
                }
                None => return Err("-j/--jobs requires a value (e.g. `-j 4`)".to_string()),
            }
        } else {
            rest.push(a.clone());
        }
        i += 1;
    }
    Ok((jobs, rest))
}

/// Resolve the codegen worker count: the `-j`/`--jobs` flag wins, else `ALIGNC_JOBS`, else
/// [`std::thread::available_parallelism`] (1 if that is unavailable). A malformed `ALIGNC_JOBS` is a
/// hard error (surfaced by the caller) — never a silent fallback.
fn resolve_jobs(flag: Option<usize>) -> Result<usize, String> {
    if let Some(n) = flag {
        return Ok(n);
    }
    if let Some(v) = std::env::var_os("ALIGNC_JOBS") {
        let s = v.to_string_lossy();
        return match s.trim().parse::<usize>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(format!("invalid ALIGNC_JOBS '{s}' (expected a positive integer)")),
        };
    }
    Ok(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
}

/// Validate the `--export` roots against the **entry unit** (M15 S2b): `--export` is entry-unit-only
/// — every root must name a function defined in the entry unit's MIR, applied only to the entry
/// unit's object. Fail-closed, three outcomes per unresolved name:
///   * defined in the entry unit → OK (kept external in the entry object).
///   * defined in a *non-entry* unit → hard error naming that unit. `--export` cannot reach it; a
///     non-entry `pub` function is already external (that is the one way to export it), so the fix is
///     to mark it `pub`, not to `--export` it.
///   * defined nowhere → the listed unknown-export error (a typo'd name never silently no-ops).
///
/// Returns the failing `ExitCode` on any rejection, `None` when every root resolves in the entry unit.
fn check_exports_entry(walk: &PerUnitWalk, exports: &[String], path: &str) -> Option<ExitCode> {
    if exports.is_empty() {
        return None;
    }
    let Some(entry) = walk.units.iter().find(|u| u.is_entry) else {
        // A clean walk always compiles its entry, so this is unreachable after `walk_or_report`;
        // fail closed rather than silently drop the exports if it ever is not.
        eprintln!("alignc: cannot apply --export: no entry unit was compiled");
        return Some(ExitCode::FAILURE);
    };
    let not_in_entry = unknown_exports(&entry.mir, exports);
    if not_in_entry.is_empty() {
        return None;
    }
    // A non-entry unit `u` mangles its functions `u$name`; match the source name against that suffix
    // (or the bare name defensively) to tell "defined in another unit" apart from "defined nowhere".
    let mut unknown: Vec<&str> = Vec::new();
    let mut rejected = false;
    for name in not_in_entry {
        let suffix = format!("${name}");
        if let Some(u) = walk
            .units
            .iter()
            .find(|u| !u.is_entry && u.mir.fns.iter().any(|f| f.name == name || f.name.ends_with(&suffix)))
        {
            rejected = true;
            eprintln!(
                "alignc: --export '{name}' names a function defined in unit '{u}', not the entry unit; \
                 --export applies only to the entry unit. Mark it `pub` in `{u}` to export it \
                 (a non-entry `pub` function already has external linkage).",
                u = u.unit
            );
        } else {
            unknown.push(name);
        }
    }
    if !unknown.is_empty() {
        eprintln!("alignc: unknown export(s): {} (not defined in {path})", unknown.join(", "));
        rejected = true;
    }
    rejected.then_some(ExitCode::FAILURE)
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
        "usage: alignc <command> <file.align> [--target-cpu baseline|native]\n  \
                alignc --version\n\
         \n\
         commands:\n  \
           check      check through lexer/parser/sema\n  \
           check-per-unit  check each unit against its imports' interface summaries (M15 S1b)\n  \
           emit-interface  print each unit's interface summary + interface/impl hashes\n  \
           emit-mir   print MIR as text\n  \
           emit-llvm  print LLVM IR as text (--stage raw|optimized; default raw)\n  \
           emit-obj   write an object file (<file> [out.o]; no link, no `main` needed)\n  \
           explain-opt report what the -O2 optimizer did to the data path (--verbose for detail)\n  \
           fmt        format source (prints to stdout; --write rewrites in place)\n  \
           build      build an executable\n  \
           run        build and run (returns the exit code)\n  \
           size       build then report the executable's size breakdown\n  \
           cache clear  remove the codegen cache under the resolved ALIGNC_CACHE root\n  \
         \n\
         --target-cpu  baseline (default; portable per-arch floor), native (this host's CPU),\n  \
                       or an LLVM CPU name like x86-64-v3 (a portable fast tier for a known fleet)\n  \
         --profile     dev (O0), release (O2, default), fast (O3), small (Os), tiny (Oz)\n  \
         --export      (emit-obj/emit-llvm only; repeatable) keep an entry-file top-level function\n  \
                       name's linkage external instead of the default internal, so a no-`main`\n  \
                       library/benchmark object exposes it to the linker\n  \
         --rt-lto      (build/run/emit-obj/size/emit-llvm; release/fast only) link the fast-path\n  \
                       string primitives' bitcode into the program and inline it before the opt run\n  \
         --thin-lto    (build/run/size; release/fast only) cross-unit ThinLTO — optimize across the\n  \
                       per-unit boundary (serial, no cache in v1); composes with --rt-lto\n  \
         --pgo-instrument (build/run/size; release/fast only) build a profile-generating binary; run\n  \
                       it to write a .profraw, `llvm-profdata-22 merge` it, then rebuild with --pgo-use\n  \
         --pgo-use F   (build/run/size; release/fast only) rebuild using merged profile data F\n  \
                       (.profdata); exclusive with --pgo-instrument; not combinable with --thin-lto\n  \
         --cache-stats (build/run/size) print a per-unit codegen-cache hit/miss report\n  \
         -j, --jobs N  (build/run/size) codegen worker threads (default: available parallelism;\n  \
                       overrides ALIGNC_JOBS)\n  \
         \n\
         ALIGNC_CACHE  on | <path> | off — the codegen cache (default: on, at the XDG cache root)\n  \
         ALIGNC_JOBS   default codegen worker-thread count (the -j flag overrides it)"
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

/// Run the per-unit walk for `path` (front end → per-unit sema → per-unit MIR, bottom-up over the
/// import DAG), printing any diagnostics. Returns the walk on success (at least one unit), or `None`
/// on a read/parse/check error (diagnostics already emitted). This is the shared front half of every
/// codegen verb (`build`/`run`/`size`/`emit-obj`/`emit-llvm`/`emit-mir`) after the M15 S2b flip.
fn walk_or_report(path: &str) -> Option<PerUnitWalk> {
    let src = read(path)?;
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, path, &src);
    if !walk.diags.is_empty() {
        eprint!("{}", format_diagnostics(&sm, &walk.diags));
    }
    if walk.diags.has_errors() {
        return None;
    }
    if walk.units.is_empty() {
        eprintln!("alignc: no units to build");
        return None;
    }
    Some(walk)
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

/// `alignc check-per-unit <file>` (M15 S1b, dev verb): check the program **per unit** — each unit
/// against only its own AST plus the interface summaries of its (transitively-closed) imports, walking
/// the import DAG bottom-up. Prints each unit's transitive interface-hash dependency set (the S3 cache
/// key input). This is an additive capability that proves the separate-compilation seam; it does not
/// replace the whole-program `check`/`build` path.
fn run_check_per_unit(path: &str) -> ExitCode {
    let Some(src) = read(path) else {
        return ExitCode::FAILURE;
    };
    let mut sm = SourceMap::new();
    let result = align_driver::check_per_unit(&mut sm, path, &src);
    if !result.diags.is_empty() {
        eprint!("{}", format_diagnostics(&sm, &result.diags));
    }
    if result.diags.has_errors() {
        return ExitCode::FAILURE;
    }
    for (unit, deps) in &result.dep_interface_hashes {
        println!("unit {unit}");
        if deps.is_empty() {
            println!("  (no dependencies)");
        }
        for (dep, hash) in deps {
            println!("  depends on {dep} @ {}", hash.to_hex());
        }
    }
    println!("ok: checked {} unit(s) per-unit", result.dep_interface_hashes.len());
    ExitCode::SUCCESS
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
    let Some(walk) = walk_or_report(path) else {
        return ExitCode::FAILURE;
    };
    // Per unit, bottom-up. N=1 prints exactly the single unit's MIR (byte-identical to the pre-flip
    // whole-program dump — a single-file program's per-unit MIR equals its whole-program MIR). N>1
    // precedes each unit with a banner comment so the units are distinguishable in one stream.
    let multi = walk.units.len() > 1;
    let mut out = String::new();
    for unit in &walk.units {
        if multi {
            out.push_str(&format!("// ==== unit: {} ====\n", unit.unit));
        }
        out.push_str(&align_mir::print::program_to_string(&unit.mir));
    }
    print!("{out}");
    ExitCode::SUCCESS
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
    let Some(walk) = walk_or_report(path) else {
        return ExitCode::FAILURE;
    };
    // `--export` is entry-unit-only (validated against the entry unit's MIR; applied only to it).
    if let Some(code) = check_exports_entry(&walk, exports, path) {
        return code;
    }
    // Each unit is optimized in isolation (that is the truth under zero cross-unit optimization): a
    // cross-unit `pub` call stays an opaque call, while an intra-unit call inlines. N=1 = byte-
    // identical to the pre-flip whole-program IR; N>1 banners each unit.
    let multi = walk.units.len() > 1;
    let mut out = String::new();
    for unit in &walk.units {
        let unit_exports: &[String] = if unit.is_entry { exports } else { &[] };
        let ir = match emit_llvm_ir(&unit.mir, target.clone(), optimized, unit_exports, rt_lto) {
            Ok(ir) => ir,
            Err(e) => {
                eprintln!("alignc: {e}");
                return ExitCode::FAILURE;
            }
        };
        if multi {
            out.push_str(&format!("; ==== unit: {} ====\n", unit.unit));
        }
        out.push_str(&ir);
    }
    print!("{out}");
    ExitCode::SUCCESS
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
    let Some(walk) = walk_or_report(path) else {
        return ExitCode::FAILURE;
    };
    // `--export` is entry-unit-only (validated against the entry unit's MIR; applied only to it).
    if let Some(code) = check_exports_entry(&walk, exports, path) {
        return code;
    }
    // Opt-in codegen cache (ALIGNC_CACHE); `--export` folds into the key. Disabled ⇒ verbatim emit.
    let cache = CacheContext::from_env();
    if let [unit] = walk.units.as_slice() {
        // N=1: byte-identical to the pre-flip whole-program object — `<stem>.o` (or the given output
        // path), with any `--export` applied to the single (entry) unit.
        let obj = PathBuf::from(out.map(String::from).unwrap_or_else(|| format!("{}.o", stem(path))));
        return match emit_object_cached(
            &cache,
            &unit.unit,
            unit.summary.impl_hash,
            &unit.dep_interface_hashes,
            &unit.mir,
            &obj,
            target,
            profile,
            exports,
            rt_lto,
        ) {
            Ok(_) => {
                println!("alignc: wrote object: {}", obj.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("alignc: codegen failed: {e}");
                ExitCode::FAILURE
            }
        };
    }
    // N>1: one object per unit, named `<module-path>.o` in the current directory. A single `[out.o]`
    // positional is ambiguous (it can name only one of N objects) — a hard error with guidance, never
    // a silent pick of one unit.
    if let Some(out) = out {
        eprintln!(
            "alignc: a multi-unit program emits one object per unit ('<module>.o'); \
             omit the output path (got '{out}')"
        );
        return ExitCode::FAILURE;
    }
    for unit in &walk.units {
        let obj = PathBuf::from(format!("{}.o", unit.unit));
        // Exports apply only to the entry unit (a non-entry `pub` fn is already external via per-unit
        // lowering); every other unit emits with no export roots.
        let unit_exports: &[String] = if unit.is_entry { exports } else { &[] };
        if let Err(e) = emit_object_cached(
            &cache,
            &unit.unit,
            unit.summary.impl_hash,
            &unit.dep_interface_hashes,
            &unit.mir,
            &obj,
            target.clone(),
            profile,
            unit_exports,
            rt_lto,
        ) {
            eprintln!("alignc: codegen failed for unit `{}`: {e}", unit.unit);
            return ExitCode::FAILURE;
        }
        println!("alignc: wrote object: {}", obj.display());
    }
    ExitCode::SUCCESS
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

/// Compile `path` **per unit** (walk the import DAG bottom-up, one object per unit under the
/// separate-compilation visibility model) and link the N objects into `exe`. The one build path for
/// `build`/`run`/`size` after the M15 S2b flip. Objects stage in a process-private directory (named
/// by a per-unit index, not the `.`-containing module path); capability libraries are unioned
/// deterministically first-seen across units; the executable is published to `exe` by same-directory
/// atomic rename. Returns the failing `ExitCode` (diagnostics already printed) on any error.
#[allow(clippy::too_many_arguments)]
fn build_per_unit_to(path: &str, exe: &Path, target: BuildTarget, profile: Profile, rt_lto: bool, thin_lto: bool, pgo: &align_driver::PgoMode, jobs: usize, cache_stats: bool) -> Result<(), ExitCode> {
    let walk = walk_or_report(path).ok_or(ExitCode::FAILURE)?;
    let object_stage = ArtifactStage::temp("align-per-unit-obj").map_err(|e| {
        eprintln!("alignc: cannot create object staging directory: {e}");
        ExitCode::FAILURE
    })?;
    // One object path per unit (DAG-index-named, not the `.`-containing module path).
    let obj_paths: Vec<PathBuf> = (0..walk.units.len()).map(|i| object_stage.path().join(format!("unit{i}.o"))).collect();
    // Opt-in codegen cache (ALIGNC_CACHE), default-ON; disabled ⇒ each unit emits verbatim.
    let cache = CacheContext::from_env();
    if thin_lto && walk.units.len() >= 2 {
        // ThinLTO S2: cross-unit-optimizing build with two cacheable phases per unit (prelink bitcode
        // + backend object) and a serial thin-link between them; misses run in parallel (`jobs`
        // workers). Fail-closed on any shim failure — NEVER a silent fallback to the non-ThinLTO path
        // (the user asked for --thin-lto). N=1 skips ThinLTO and falls through to the ordinary object
        // cache below (byte-identical to today's whole-program object, one shared key namespace).
        let outcomes = match align_driver::build_thin_lto(
            &walk.units, &obj_paths, &cache, &target, profile, &[], rt_lto, object_stage.path(), jobs,
        ) {
            Ok(build) => build.outcomes,
            Err(e) => {
                eprintln!("alignc: {e}");
                return Err(ExitCode::FAILURE);
            }
        };
        if cache_stats {
            render_thin_cache_stats(&outcomes, cache.is_enabled());
        }
        return finish_link(&walk, &obj_paths, exe, profile, &target, &align_driver::PgoMode::Off);
    }
    // Instrument-PGO (`--pgo-instrument` / `--pgo-use`, S2) now flows through the NORMAL cached +
    // parallel per-unit path below — the object cache composes it via the `PgoKey` key component
    // (instrumented / profile-use / ordinary objects are structurally isolated and never share a CAS
    // blob). Only two PGO-specific bits remain: the per-unit emit swaps in the PGO pipeline
    // (`codegen_units_parallel` → `emit_object_pgo`), and the link pulls the profile runtime
    // (`finish_link` under `--pgo-instrument`). Fail-loud profdata validation runs HERE, before codegen,
    // so a missing/corrupt profile is a clean CLI error rather than a libLLVM diagnose-and-exit (the S1
    // caveat) — even on an all-hit build where no LLVM would otherwise run.
    if let align_driver::PgoMode::Use(p) = pgo
        && let Err(e) = align_driver::validate_profdata(p)
    {
        eprintln!("alignc: {e}");
        return Err(ExitCode::FAILURE);
    }
    // Codegen runs in parallel over cache MISSES (`jobs` workers); lookups are serial and results stay
    // DAG-ordered. This is also the N=1 `--thin-lto` path (a single unit has no cross-unit boundary).
    let build = match align_driver::codegen_units_parallel(&walk.units, &obj_paths, &cache, &target, profile, rt_lto, jobs, pgo) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("alignc: {e}");
            return Err(ExitCode::FAILURE);
        }
    };
    if cache_stats {
        render_cache_stats(&build.outcomes, cache.is_enabled());
    }
    // One aggregated Align-voice `--pgo-use` report over the units that actually ran (cache MISSES), then
    // proceed — a mismatched profile is a PERFORMANCE concern, never a correctness one (clang parity), so
    // it is a WARNING, never an abort. A `matched == 0` build (the profile applied to NOTHING) gets a
    // prominent "is this the right profile?" line; a partial match rides the per-unit staleness warnings.
    // Hard fails stay at the reliable layer (missing/bad-magic profdata; an Error-severity libLLVM
    // diagnostic), handled before/inside codegen. An all-hit build ran no LLVM, so has a `0/0` tally and no
    // warnings: any staleness was reported when each object was first built and is intrinsic to the bytes.
    if matches!(pgo, align_driver::PgoMode::Use(_)) {
        if build.pgo_total > 0 && build.pgo_matched == 0 {
            eprintln!(
                "alignc: --pgo-use: the profile matched 0 of {} rebuilt function(s) — is this profile \
                 from this program? Proceeding without profile guidance (this affects performance only, \
                 never correctness).",
                build.pgo_total
            );
        } else if !build.pgo_warnings.is_empty() {
            eprintln!(
                "alignc: --pgo-use: proceeding despite {} PGO profile-use warning(s) across the rebuilt \
                 unit(s) ({} of {} function(s) matched the profile; the rest changed since it was \
                 collected); first: {}",
                build.pgo_warnings.len(),
                build.pgo_matched,
                build.pgo_total,
                build.pgo_warnings[0]
            );
        }
    }
    finish_link(&walk, &obj_paths, exe, profile, &target, pgo)
}

/// Link the per-unit objects into `exe`: the deterministic capability-library union (first-seen in
/// DAG order) + link + atomic-rename publish. Shared by the normal cached path and the `--thin-lto`
/// path (the objects differ; the link step is identical).
fn finish_link(walk: &PerUnitWalk, obj_paths: &[PathBuf], exe: &Path, profile: Profile, target: &BuildTarget, pgo: &align_driver::PgoMode) -> Result<(), ExitCode> {
    // Deterministic capability union across units, first-seen in DAG (unit) order — never completion
    // order (parallel codegen may finish units out of order, but this iterates `walk.units`).
    let mut link_libs: Vec<String> = Vec::new();
    for unit in &walk.units {
        for lib in &unit.mir.link_libs {
            if !link_libs.contains(lib) {
                link_libs.push(lib.clone());
            }
        }
    }

    let parent = exe.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let publish_stage = ArtifactStage::in_dir(parent, "align-publish").map_err(|e| {
        eprintln!("alignc: cannot create executable staging directory: {e}");
        ExitCode::FAILURE
    })?;
    let staged_exe = publish_stage.path().join(exe.file_name().unwrap_or_else(|| std::ffi::OsStr::new("program")));
    let obj_refs: Vec<&Path> = obj_paths.iter().map(|p| p.as_path()).collect();
    // Under `--pgo-instrument` the link additionally pulls the clang profile runtime and forces the
    // `__llvm_profile_runtime` anchor undefined (so the atexit `.profraw` writer survives) — and, per
    // Nothing-hidden, PRINTS where the running binary will write its profile. `--pgo-use` links
    // ordinarily (the profile is already baked into the optimized objects).
    let link_result = if matches!(pgo, align_driver::PgoMode::Instrument) {
        let profile_rt = match align_driver::profile_runtime_archive(target) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("alignc: {e}");
                return Err(ExitCode::FAILURE);
            }
        };
        let dest = std::env::var("LLVM_PROFILE_FILE").unwrap_or_else(|_| "default.profraw".to_string());
        // Surfaced on stderr, not stdout: under `run` the built program's own stdout must stay clean,
        // and `size` parses stdout — this is a diagnostic note, so it belongs on stderr.
        eprintln!(
            "alignc: --pgo-instrument: instrumented binary will write its profile to `{dest}` when run \
             (set LLVM_PROFILE_FILE to redirect); then `llvm-profdata-22 merge` it and rebuild with \
             `--pgo-use <file.profdata>`"
        );
        align_driver::link_objects_instrumented(&obj_refs, &staged_exe, &link_libs, profile, &profile_rt)
    } else {
        link_objects(&obj_refs, &staged_exe, &link_libs, profile)
    };
    if let Err(e) = link_result {
        eprintln!("alignc: {e}");
        return Err(ExitCode::FAILURE);
    }
    if let Err(e) = std::fs::rename(&staged_exe, exe) {
        eprintln!("alignc: cannot publish executable {}: {e}", exe.display());
        return Err(ExitCode::FAILURE);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_build(path: &str, target: BuildTarget, profile: Profile, rt_lto: bool, thin_lto: bool, pgo: &align_driver::PgoMode, jobs: usize, cache_stats: bool) -> ExitCode {
    let exe = PathBuf::from(stem(path));
    match build_per_unit_to(path, &exe, target, profile, rt_lto, thin_lto, pgo, jobs, cache_stats) {
        Ok(()) => {
            println!("alignc: built executable: {}", exe.display());
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

/// Render the `--cache-stats` report: one `hit` / `miss (<reason>)` line per unit + a summary count.
/// Silent on all-hit is the *default* (no flag); with the flag we always print. A disabled cache
/// prints a single note (there are no per-unit lookups to report).
fn render_cache_stats(outcomes: &[align_driver::CacheOutcome], enabled: bool) {
    if !enabled {
        eprintln!("alignc: cache: disabled (set ALIGNC_CACHE=on or a path to enable)");
        return;
    }
    let (mut hits, mut misses) = (0usize, 0usize);
    for o in outcomes {
        if o.hit {
            hits += 1;
            eprintln!("alignc: cache: {} hit", o.unit);
        } else {
            misses += 1;
            let reason = o.miss_reason.map(|r| r.reason()).unwrap_or("miss");
            eprintln!("alignc: cache: {} miss ({reason})", o.unit);
        }
    }
    eprintln!("alignc: cache: {} unit(s): {hits} hit, {misses} miss", outcomes.len());
}

/// Render the `--cache-stats` report for a `--thin-lto` build: one `<unit> <phase> hit`/`miss (<r>)`
/// line per phase per unit (`prelink` then `backend`), then a per-phase summary. A disabled cache
/// prints the single disabled note (there are no per-unit lookups to report).
fn render_thin_cache_stats(outcomes: &[align_driver::CacheOutcome], enabled: bool) {
    if !enabled {
        eprintln!("alignc: cache: disabled (set ALIGNC_CACHE=on or a path to enable)");
        return;
    }
    for stage in [align_driver::CacheStage::ThinLtoPrelink, align_driver::CacheStage::ThinLtoBackend] {
        let (mut hits, mut misses) = (0usize, 0usize);
        for o in outcomes.iter().filter(|o| o.stage == stage) {
            if o.hit {
                hits += 1;
                eprintln!("alignc: cache: {} {} hit", o.unit, stage.label());
            } else {
                misses += 1;
                let reason = o.miss_reason.map(|r| r.reason()).unwrap_or("miss");
                eprintln!("alignc: cache: {} {} miss ({reason})", o.unit, stage.label());
            }
        }
        eprintln!("alignc: cache: {} {}: {hits} hit, {misses} miss", hits + misses, stage.label());
    }
}

/// `alignc cache clear` — remove the cache-owned subtrees (`cas`/`actions`/`index`) under the resolved
/// cache root. Honors `ALIGNC_CACHE` path resolution (an explicit path, else the default XDG root),
/// even when the cache is currently disabled. Safe on an absent root.
fn run_cache_clear() -> ExitCode {
    let Some(root) = CacheContext::clear_root() else {
        eprintln!("alignc: cannot resolve the cache root (set ALIGNC_CACHE or HOME/XDG_CACHE_HOME)");
        return ExitCode::FAILURE;
    };
    match align_driver::clear_cache(&root) {
        Ok(true) => {
            println!("alignc: cleared cache under {}", root.display());
            ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("alignc: cache already empty under {}", root.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("alignc: {e}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_run(path: &str, prog_args: &[String], target: BuildTarget, profile: Profile, rt_lto: bool, thin_lto: bool, pgo: &align_driver::PgoMode, jobs: usize, cache_stats: bool) -> ExitCode {
    let stage = match ArtifactStage::temp("align-run") {
        Ok(stage) => stage,
        Err(e) => {
            eprintln!("alignc: cannot create run staging directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let exe = stage.path().join("program");
    if let Err(code) = build_per_unit_to(path, &exe, target, profile, rt_lto, thin_lto, pgo, jobs, cache_stats) {
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
