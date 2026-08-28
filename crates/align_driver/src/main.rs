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
//!   alignc fmt       <file>   Format source (`--write` rewrites in place)
//!   alignc build     <file>   Build an executable (<stem> in cwd)
//!   alignc run       <file>   Build, run, and return its exit code
//!   alignc size      <file>   Build then report the executable's size breakdown
//!   alignc cache clear        Remove the resolved codegen cache
//!   alignc db prepare <file>  Regenerate checked database metadata
//!   alignc db migrate/status/check/repair  Operate an explicit SQL migration catalog
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
use std::ffi::OsString;
use std::process::ExitCode;

/// The compiler is an allocation-heavy workload; mimalloc measurably beats the
/// system allocator for it (same reason rustc ships jemalloc). Declared on the
/// `alignc` binary only: in-process library consumers keep the default
/// allocator, while tests that spawn the real binary (`CARGO_BIN_EXE_alignc`)
/// exercise the shipped configuration, mimalloc included.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use align_driver::{
    build_interface_summaries, build_per_unit, check, emit_llvm_ir, emit_object_cached,
    format_diagnostics, link_objects, unknown_exports, BuildTarget, CacheContext, PerUnitWalk,
    Profile, UnitReuse,
};
use align_span::SourceMap;

mod size;
mod watch;
mod watch_native;

fn main() -> ExitCode {
    let raw_os = std::env::args_os().collect::<Vec<_>>();
    if raw_os.get(1).is_some_and(|value| value == "db") {
        if raw_os.iter().skip(2).any(|value| value == "--watch") {
            eprintln!("alignc: --watch is only valid for `build` (got `db`)");
            return ExitCode::FAILURE;
        }
        return match raw_os.get(2).and_then(|value| value.to_str()) {
            Some("prepare") => run_db_prepare(&raw_os[3..]),
            Some("migrate") => run_db_migration(DbMigrationCommand::Migrate, &raw_os[3..]),
            Some("status") => run_db_migration(DbMigrationCommand::Status, &raw_os[3..]),
            Some("check") => run_db_migration(DbMigrationCommand::Check, &raw_os[3..]),
            Some("repair") => run_db_migration(DbMigrationCommand::Repair, &raw_os[3..]),
            Some(other) => {
                eprintln!("alignc: unknown `db` subcommand `{other}` (expected: prepare, migrate, status, check, or repair)");
                ExitCode::FAILURE
            }
            None => {
                eprintln!("alignc: `db` requires a UTF-8 subcommand");
                ExitCode::FAILURE
            }
        };
    }
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
    // verb as the profile value. Cached/parallel per unit, release/fast only.
    let (pgo, args) = match parse_pgo(&raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("alignc: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (watch, args) = match parse_watch(&args) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("alignc: {error}");
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
    // Pull the `--rt-lto` / `--no-rt-lto` override (M14 Slice 2; default flipped 2026-08-09):
    // in-process link of the fast-path string primitives' bitcode into the program module before
    // the one opt run. Defaults ON for the optimizing `release`/`fast` profiles (measured 2.1x
    // aarch64 / 2.9x x86-64 on string-predicate kernels, non-regressing numeric control, +1-2ms
    // compile) and OFF for `dev`/`small`/`tiny`; the flags force either direction explicitly.
    let (rt_lto_flag, args) = parse_rt_lto(&args);
    // Pull the boolean `--thin-lto` flag: opt-in cross-unit optimization. Its prelink and backend
    // phases are cached and parallel; release/fast only; composable with `--rt-lto`.
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

    if watch && cmd != Some("build") {
        eprintln!("alignc: --watch is only valid for `build` (got `{}`)", cmd.unwrap_or("<none>"));
        return ExitCode::FAILURE;
    }
    if cmd == Some("build") && path.is_some_and(|path| path == "--help") {
        build_usage();
        return ExitCode::SUCCESS;
    }

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

    // The explicit flags only mean something where codegen runs the optimizer over a real
    // build/lens; a defaulted value is simply unused elsewhere (`check` etc. never read it).
    if rt_lto_flag.is_some()
        && !matches!(
            cmd,
            Some("build") | Some("run") | Some("emit-obj") | Some("size") | Some("emit-llvm")
        )
    {
        eprintln!(
            "alignc: --rt-lto/--no-rt-lto are only valid for `build`/`run`/`emit-obj`/`size`/`emit-llvm` (got `{}`)",
            cmd.unwrap_or("<none>")
        );
        return ExitCode::FAILURE;
    }
    // `dev` is O0 (nothing inlines, so LTO buys nothing); `small`/`tiny` run the `optsize`/
    // `minsize` sweep, which conflicts with fast-path inlining. An explicit `--rt-lto` there is
    // rejected rather than silently no-opped; the default resolves to OFF for those profiles.
    if rt_lto_flag == Some(true) && matches!(profile, Profile::Dev | Profile::Small | Profile::Tiny) {
        eprintln!(
            "alignc: --rt-lto is incompatible with the `{}` profile (it needs an inlining \
             pipeline; use `release` or `fast`)",
            profile.name()
        );
        return ExitCode::FAILURE;
    }
    let rt_lto = rt_lto_flag.unwrap_or_else(|| align_driver::default_rt_lto(profile));

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
        (Some("build"), Some(p)) if watch => {
            watch::run_watch_build(p, target, profile, rt_lto, thin_lto, &pgo, jobs, cache_stats)
        }
        (Some("build"), Some(p)) => run_build(p, target, profile, rt_lto, thin_lto, &pgo, jobs, cache_stats),
        // `run` forwards any trailing arguments to the built program (its `main(args)`).
        (Some("run"), Some(p)) => run_run(p, &args[3..], target, profile, rt_lto, thin_lto, &pgo, jobs, cache_stats),
        _ => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn parse_watch(args: &[String]) -> Result<(bool, Vec<String>), &'static str> {
    let mut watch = false;
    let mut rest = Vec::with_capacity(args.len());
    for argument in args {
        if argument == "--watch" {
            watch = true;
        } else if argument.starts_with("--watch=") {
            return Err("--watch does not take a value");
        } else {
            rest.push(argument.clone());
        }
    }
    Ok((watch, rest))
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

/// Pull the `--rt-lto` / `--no-rt-lto` override out of `args` (M14 Slice 2; profile-based default
/// since 2026-08-09), returning `Some(true)` / `Some(false)` when a flag is present (`None` = use
/// the profile default) and the remaining arguments. Valueless flags; repeated occurrences are
/// idempotent and the last occurrence wins when both appear.
fn parse_rt_lto(args: &[String]) -> (Option<bool>, Vec<String>) {
    let mut rt_lto = None;
    let mut rest = Vec::new();
    for a in args {
        if a == "--rt-lto" {
            rt_lto = Some(true);
        } else if a == "--no-rt-lto" {
            rt_lto = Some(false);
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
            .find(|u| {
                !u.is_entry
                    && u.mir.fns.iter().any(|f| {
                        f.name.as_str() == name || f.name.as_str().ends_with(&suffix)
                    })
            })
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum DbPrepareDriver {
    Sqlite,
    Postgres,
}

struct DbPrepareOptions {
    entry: String,
    driver: DbPrepareDriver,
    queries: Vec<String>,
    check_only: bool,
    database: Option<String>,
    schema_id: Option<String>,
    memory: bool,
    migrations: Option<String>,
    url_env: Option<String>,
}

fn db_text(value: &OsString, what: &str) -> Result<String, String> {
    let value = value.to_str().ok_or_else(|| format!("{what} must be UTF-8"))?;
    if value.is_empty() || value.as_bytes().contains(&0) {
        return Err(format!("{what} must be non-empty and contain no U+0000"));
    }
    Ok(value.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DbMigrationCommand {
    Migrate,
    Status,
    Check,
    Repair,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DbMigrationDriver {
    Sqlite,
    Postgres,
}

struct DbMigrationOptions {
    entry: String,
    migrations: String,
    driver: DbMigrationDriver,
    sqlite_path: Option<String>,
    postgres_url_env: Option<String>,
    version: Option<u32>,
    action: Option<align_driver::db_migrate_native::RepairAction>,
    expected_checksum: Option<String>,
}

fn parse_db_migration(
    command: DbMigrationCommand,
    args: &[OsString],
) -> Result<DbMigrationOptions, String> {
    let mut entry = None;
    let mut migrations = None;
    let mut driver_value = None;
    let mut sqlite_path = None;
    let mut postgres_url_env = None;
    let mut version_value = None;
    let mut action = None;
    let mut expected_checksum = None;
    let mut index = 0usize;
    while index < args.len() {
        let flag = db_text(&args[index], &format!("db migration argument {}", index + 1))?;
        index += 1;
        if matches!(flag.as_str(), "--accept-applied" | "--clear-dirty") {
            if action.is_some() {
                return Err("repair requires exactly one action".to_string());
            }
            action = Some(if flag == "--accept-applied" {
                align_driver::db_migrate_native::RepairAction::AcceptApplied
            } else {
                align_driver::db_migrate_native::RepairAction::ClearDirty
            });
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--entry"
                | "--migrations"
                | "--driver"
                | "--sqlite-path"
                | "--postgres-url-env"
                | "--version"
                | "--expect-checksum"
        ) {
            return Err(format!("unknown `db` migration option `{flag}`"));
        }
        let value = args
            .get(index)
            .ok_or_else(|| format!("{flag} requires a value"))
            .and_then(|value| db_text(value, &format!("db migration argument {}", index + 1)))?;
        if value.starts_with("--") {
            return Err(format!("{flag} requires a value"));
        }
        index += 1;
        match flag.as_str() {
            "--entry" if entry.is_none() => entry = Some(value.clone()),
            "--migrations" if migrations.is_none() => migrations = Some(value.clone()),
            "--driver" if driver_value.is_none() => driver_value = Some(value.clone()),
            "--sqlite-path" if sqlite_path.is_none() => sqlite_path = Some(value.clone()),
            "--postgres-url-env" if postgres_url_env.is_none() => {
                postgres_url_env = Some(value.clone())
            }
            "--version" if version_value.is_none() => version_value = Some(value.clone()),
            "--expect-checksum" if expected_checksum.is_none() => {
                expected_checksum = Some(value.clone())
            }
            _ => return Err(format!("duplicate {flag}")),
        }
    }
    let entry = entry.ok_or_else(|| "db migration requires --entry ENTRY".to_string())?;
    let migrations = migrations.ok_or_else(|| "db migration requires --migrations DIR".to_string())?;
    let driver = match driver_value
        .as_deref()
        .ok_or_else(|| "db migration requires --driver sqlite|postgres".to_string())?
    {
        "sqlite" => DbMigrationDriver::Sqlite,
        "postgres" => DbMigrationDriver::Postgres,
        _ => return Err("--driver must be `sqlite` or `postgres`".to_string()),
    };
    match driver {
        DbMigrationDriver::Sqlite => {
            if sqlite_path.is_none() || postgres_url_env.is_some() {
                return Err("SQLite migration requires exactly --sqlite-path PATH".to_string());
            }
        }
        DbMigrationDriver::Postgres => {
            if postgres_url_env.is_none() || sqlite_path.is_some() {
                return Err("PostgreSQL migration requires exactly --postgres-url-env NAME".to_string());
            }
            let name = postgres_url_env.as_deref().expect("checked above");
            let mut bytes = name.bytes();
            if name.starts_with("PG")
                || !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err("--postgres-url-env must be a non-PG* environment identifier".to_string());
            }
        }
    }
    if command == DbMigrationCommand::Repair {
        let value = version_value
            .as_deref()
            .ok_or_else(|| "db repair requires --version N".to_string())?;
        let version = value
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=9999).contains(value))
            .ok_or_else(|| "--version must be an integer from 1 through 9999".to_string())?;
        if action.is_none() {
            return Err("db repair requires exactly one action".to_string());
        }
        let checksum = expected_checksum
            .as_deref()
            .ok_or_else(|| "db repair requires --expect-checksum HASH".to_string())?;
        if checksum.len() != 32
            || !checksum
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err("--expect-checksum must be exactly 32 lowercase hexadecimal bytes".to_string());
        }
        return Ok(DbMigrationOptions {
            entry,
            migrations,
            driver,
            sqlite_path,
            postgres_url_env,
            version: Some(version),
            action,
            expected_checksum,
        });
    } else {
        if version_value.is_some() {
            return Err("--version is valid only for db repair".to_string());
        }
        if action.is_some() {
            return Err("repair actions are valid only for db repair".to_string());
        }
        if expected_checksum.is_some() {
            return Err("--expect-checksum is valid only for db repair".to_string());
        }
    }
    Ok(DbMigrationOptions {
        entry,
        migrations,
        driver,
        sqlite_path,
        postgres_url_env,
        version: None,
        action,
        expected_checksum,
    })
}

fn run_db_migration(command: DbMigrationCommand, args: &[OsString]) -> ExitCode {
    use align_driver::db_migrate::{
        resolve_migration_paths, resolve_sqlite_target, screen_postgres_catalog,
    };
    use align_driver::db_migrate_native::{
        run_postgres_migration, run_sqlite_migration, screen_sqlite_catalog_native,
        validate_postgres_migration_environment, validate_postgres_migration_url,
        MigrationOperation,
    };
    use align_driver::db_prepare::read_migration_catalog;

    let options = match parse_db_migration(command, args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("alignc: {error}");
            return ExitCode::FAILURE;
        }
    };
    let paths = match resolve_migration_paths(Path::new(&options.entry), Path::new(&options.migrations)) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("alignc: {error}");
            return ExitCode::FAILURE;
        }
    };
    let source = match std::fs::read_to_string(&paths.entry) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("alignc: cannot read `{}`: {error}", paths.entry.display());
            return ExitCode::FAILURE;
        }
    };
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &paths.entry.display().to_string(), &source);
    if walk.diags.has_errors() {
        eprint!("{}", format_diagnostics(&source_map, &walk.diags));
        return ExitCode::FAILURE;
    }
    let catalog = match read_migration_catalog(&paths.migrations) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("alignc: {error}");
            return ExitCode::FAILURE;
        }
    };
    let operation = match command {
        DbMigrationCommand::Migrate => MigrationOperation::Migrate,
        DbMigrationCommand::Status => MigrationOperation::Status,
        DbMigrationCommand::Check => MigrationOperation::Check,
        DbMigrationCommand::Repair => MigrationOperation::Repair {
            version: options.version.expect("validated repair version"),
            action: options.action.expect("validated repair action"),
            expected_checksum: options
                .expected_checksum
                .as_deref()
                .expect("validated repair checksum"),
        },
    };
    let result = match options.driver {
        DbMigrationDriver::Sqlite => {
            let screened = match screen_sqlite_catalog_native(&catalog) {
                Ok(screened) => screened,
                Err(error) => {
                    eprintln!("alignc: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let target = resolve_sqlite_target(
                &paths.project_root,
                Path::new(options.sqlite_path.as_deref().expect("validated SQLite target")),
            );
            run_sqlite_migration(&target, operation, &screened)
        }
        DbMigrationDriver::Postgres => {
            let screened = match screen_postgres_catalog(&catalog) {
                Ok(screened) => screened,
                Err(error) => {
                    eprintln!("alignc: {error}");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(error) = validate_postgres_migration_environment() {
                eprintln!("alignc: {error}");
                return ExitCode::FAILURE;
            }
            let name = options
                .postgres_url_env
                .as_deref()
                .expect("validated PostgreSQL environment name");
            let url = match std::env::var(name) {
                Ok(value) if !value.is_empty() && !value.as_bytes().contains(&0) => value,
                Ok(_) => {
                    eprintln!("alignc: environment variable `{name}` is empty");
                    return ExitCode::FAILURE;
                }
                Err(std::env::VarError::NotPresent) => {
                    eprintln!("alignc: environment variable `{name}` is not set");
                    return ExitCode::FAILURE;
                }
                Err(std::env::VarError::NotUnicode(_)) => {
                    eprintln!("alignc: environment variable `{name}` is not UTF-8");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(error) = validate_postgres_migration_url(&url) {
                eprintln!("alignc: {error}");
                return ExitCode::FAILURE;
            }
            run_postgres_migration(&url, operation, &screened)
        }
    };
    match result {
        Ok(report) => {
            print!("{}", report.render());
            if command == DbMigrationCommand::Check && !report.is_current() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("alignc: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_db_prepare(args: &[OsString]) -> Result<DbPrepareOptions, String> {
    let entry = args
        .first()
        .ok_or_else(|| "`db prepare` requires an entry .align path".to_string())
        .and_then(|value| db_text(value, "entry path"))?;
    if entry.starts_with("--") {
        return Err("`db prepare` requires the entry path before options".to_string());
    }
    let mut driver = None;
    let mut queries = Vec::new();
    let mut check_only = false;
    let mut database = None;
    let mut schema_id = None;
    let mut memory = false;
    let mut migrations = None;
    let mut url_env = None;
    let mut index = 1usize;
    while index < args.len() {
        let flag = args
            .get(index)
            .ok_or_else(|| "db prepare option index is out of range".to_string())
            .and_then(|value| db_text(value, "option name"))?;
        index += 1;
        if flag == "--check" {
            if check_only {
                return Err("duplicate --check".to_string());
            }
            check_only = true;
            continue;
        }
        if flag == "--memory" {
            if memory {
                return Err("duplicate --memory".to_string());
            }
            memory = true;
            continue;
        }
        let value = args
            .get(index)
            .ok_or_else(|| format!("{flag} requires a value"))
            .and_then(|value| db_text(value, &format!("{flag} value")))?;
        if value.starts_with("--") {
            return Err(format!("{flag} requires a value"));
        }
        index += 1;
        match flag.as_str() {
            "--driver" => {
                if driver.is_some() {
                    return Err("duplicate --driver".to_string());
                }
                driver = Some(match value.as_str() {
                    "sqlite" => DbPrepareDriver::Sqlite,
                    "postgres" => DbPrepareDriver::Postgres,
                    _ => return Err("--driver must be `sqlite` or `postgres`".to_string()),
                });
            }
            "--query" => {
                if queries.iter().any(|query| query == &value) {
                    return Err(format!("duplicate --query descriptor `{value}`"));
                }
                queries.push(value);
            }
            "--database" if database.is_none() => database = Some(value),
            "--schema-id" if schema_id.is_none() => schema_id = Some(value),
            "--migrations" if migrations.is_none() => migrations = Some(value),
            "--url-env" if url_env.is_none() => url_env = Some(value),
            "--database" | "--schema-id" | "--migrations" | "--url-env" => {
                return Err(format!("duplicate {flag}"));
            }
            _ => return Err(format!("unknown `db prepare` option `{flag}`")),
        }
    }
    let driver = driver.ok_or_else(|| "`db prepare` requires --driver sqlite|postgres".to_string())?;
    match driver {
        DbPrepareDriver::Sqlite => {
            let database_form = database.is_some() && schema_id.is_some() && !memory && migrations.is_none();
            let memory_form = memory && database.is_none() && schema_id.is_none();
            if !database_form && !memory_form {
                return Err(
                    "SQLite requires exactly `--database PATH --schema-id ID` or `--memory [--migrations DIR]`"
                        .to_string(),
                );
            }
            if url_env.is_some() {
                return Err("--url-env is valid only for PostgreSQL".to_string());
            }
        }
        DbPrepareDriver::Postgres => {
            if url_env.is_none() || schema_id.is_none() {
                return Err("PostgreSQL requires `--url-env NAME --schema-id ID`".to_string());
            }
            if url_env.as_deref().is_some_and(|name| name.starts_with("PG")) {
                return Err("PostgreSQL --url-env must not begin with `PG`".to_string());
            }
            if database.is_some() || memory || migrations.is_some() {
                return Err("--database, --memory, and --migrations are valid only for SQLite".to_string());
            }
        }
    }
    Ok(DbPrepareOptions {
        entry,
        driver,
        queries,
        check_only,
        database,
        schema_id,
        memory,
        migrations,
        url_env,
    })
}

fn run_db_prepare(args: &[OsString]) -> ExitCode {
    use align_driver::db_prepare::{
        build_metadata_batch, publish_metadata_batch, read_migration_catalog,
        sqlite_database_schema_fingerprint, sqlite_memory_schema_fingerprint,
    };
    use align_driver::db_prepare_native::{PostgresDescriber, SqliteDescriber};

    let options = match parse_db_prepare(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("alignc: {error}");
            return ExitCode::FAILURE;
        }
    };
    let entry = PathBuf::from(&options.entry);
    let source = match std::fs::read_to_string(&entry) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("alignc: cannot read `{}`: {error}", entry.display());
            return ExitCode::FAILURE;
        }
    };
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, &entry.display().to_string(), &source);
    if checked.diags.has_errors() {
        eprint!("{}", format_diagnostics(&source_map, &checked.diags));
        return ExitCode::FAILURE;
    }

    let result = match options.driver {
        DbPrepareDriver::Sqlite => {
            let mut describer = if let Some(database) = &options.database {
                let Some(schema_id) = options.schema_id.as_deref() else {
                    eprintln!("alignc: SQLite database preparation lost its validated --schema-id");
                    return ExitCode::FAILURE;
                };
                let fingerprint = match sqlite_database_schema_fingerprint(schema_id) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("alignc: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                SqliteDescriber::database(Path::new(database), fingerprint)
            } else if let Some(directory) = &options.migrations {
                let catalog = match read_migration_catalog(Path::new(directory)) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("alignc: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                SqliteDescriber::memory_with_migrations(catalog)
            } else {
                SqliteDescriber::memory(sqlite_memory_schema_fingerprint(None))
            };
            build_metadata_batch(
                &mut source_map,
                &entry,
                &checked,
                &options.queries,
                &mut describer,
            )
            .map(|batch| (batch, if options.memory { "memory" } else { "database" }))
        }
        DbPrepareDriver::Postgres => {
            let Some(environment_name) = options.url_env.as_deref() else {
                eprintln!("alignc: PostgreSQL preparation lost its validated --url-env");
                return ExitCode::FAILURE;
            };
            let url = match std::env::var(environment_name) {
                Ok(value) if !value.is_empty() && !value.as_bytes().contains(&0) => value,
                Ok(_) => {
                    eprintln!("alignc: environment variable `{environment_name}` is empty");
                    return ExitCode::FAILURE;
                }
                Err(std::env::VarError::NotPresent) => {
                    eprintln!("alignc: environment variable `{environment_name}` is not set");
                    return ExitCode::FAILURE;
                }
                Err(std::env::VarError::NotUnicode(_)) => {
                    eprintln!("alignc: environment variable `{environment_name}` is not UTF-8");
                    return ExitCode::FAILURE;
                }
            };
            let Some(schema_id) = options.schema_id.clone() else {
                eprintln!("alignc: PostgreSQL preparation lost its validated --schema-id");
                return ExitCode::FAILURE;
            };
            let mut describer = PostgresDescriber::new(url, schema_id);
            build_metadata_batch(
                &mut source_map,
                &entry,
                &checked,
                &options.queries,
                &mut describer,
            )
            .map(|batch| (batch, "postgres"))
        }
    };
    let (batch, source_kind) = match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("alignc: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "driver={} schema_source={} schema_identity={} engine_version={} driver_version={} queries={}",
        match batch.driver {
            align_driver::Driver::SQLite => "sqlite",
            align_driver::Driver::PostgreSQL => "postgres",
        },
        source_kind,
        batch.environment.schema_fingerprint.to_hex(),
        batch.environment.engine_version,
        batch.environment.driver_version,
        batch.files.len(),
    );
    if batch.driver == align_driver::Driver::PostgreSQL {
        println!("search_path={}", batch.environment.search_path.join(","));
        for extension in &batch.environment.extensions {
            println!(
                "extension={}.{}@{}",
                extension.schema,
                extension.name,
                extension.version.as_deref().unwrap_or("<none>")
            );
        }
    }
    match publish_metadata_batch(&batch, options.check_only) {
        Ok(report) => {
            println!("selected={} changed={}", report.selected, report.changed);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("alignc: {error}");
            ExitCode::FAILURE
        }
    }
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
           db prepare regenerate checked SQLite/PostgreSQL metadata\n  \
           db migrate apply an explicit SQLite/PostgreSQL migration catalog\n  \
           db status/check  inspect migration state; check requires exact current state\n  \
           db repair  checksum-bound repair of one dirty forbidden migration\n  \
         \n\
         --target-cpu  baseline (default; portable per-arch floor), native (this host's CPU),\n  \
                       or an LLVM CPU name like x86-64-v3 (a portable fast tier for a known fleet)\n  \
         --profile     dev (O0), release (O2, default), fast (O3), small (Os), tiny (Oz)\n  \
         --export      (emit-obj/emit-llvm only; repeatable) keep an entry-file top-level function\n  \
                       name's linkage external instead of the default internal, so a no-`main`\n  \
                       library/benchmark object exposes it to the linker\n  \
         --rt-lto      (build/run/emit-obj/size/emit-llvm) force runtime-bitcode LTO ON — the\n  \
                       default at release/fast; explicit ON still requires release/fast\n  \
         --no-rt-lto   (same verbs) force runtime-bitcode LTO OFF on any profile\n  \
         --thin-lto    (build/run/size; release/fast only) cross-unit ThinLTO — cached, parallel\n  \
                       prelink/backend phases with a serial thin-link; composes with --rt-lto\n  \
         --pgo-instrument (build/run/size; release/fast only) build a profile-generating binary; run\n  \
                       it to write a .profraw, `llvm-profdata-22 merge` it, then rebuild with --pgo-use\n  \
         --pgo-use F   (build/run/size; release/fast only) rebuild using merged profile data F\n  \
                       (.profdata); exclusive with --pgo-instrument; not combinable with --thin-lto\n  \
         --cache-stats (build/run/size) print a per-unit codegen-cache hit/miss report\n  \
         --watch       (build only) rebuild on compiler-observed file changes; other toolchain/library\n  \
                       changes need another observed change or restart\n  \
         -j, --jobs N  (build/run/size) codegen worker threads (default: available parallelism;\n  \
                       overrides ALIGNC_JOBS)\n  \
         \n\
         ALIGNC_CACHE  on | <path> | off — the codegen cache (default: on, at the XDG cache root)\n  \
         ALIGNC_JOBS   default codegen worker-thread count (the -j flag overrides it)\n  \
         ALIGNC_LINKER system | lld — pin the linker (default: lld on ELF when the LLVM\n  \
                       toolchain ships one, otherwise the system linker; macOS always uses the\n  \
                       system linker). Only link speed changes, never the optimization applied."
    );
}

fn build_usage() {
    eprintln!(
        "usage: alignc build <file.align> [build options] [--watch]\n\
         \n\
         --watch  rebuild on compiler-observed file changes; other toolchain/library changes need another observed change or restart"
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

/// Whether a package build should print the diagnostics it produced.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DiagnosticEcho {
    All,
    ErrorsOnly,
}

/// The deterministic capability-library union across units: first-seen in DAG (unit) order, never
/// completion order (parallel codegen may finish out of order). Identical for a lowered unit and a
/// reused one — a reused unit carries its `link_libs` precisely because the link never needs MIR.
fn link_lib_union<'a>(per_unit: impl Iterator<Item = &'a [String]>) -> Vec<String> {
    let mut union: Vec<String> = Vec::new();
    for libs in per_unit {
        for lib in libs {
            if !union.contains(lib) {
                union.push(lib.clone());
            }
        }
    }
    union
}

/// Run the per-unit walk for `path` (front end → per-unit sema → per-unit MIR, bottom-up over the
/// import DAG), printing any diagnostics. Returns the walk on success (at least one unit), or `None`
/// on a read/parse/check error (diagnostics already emitted). This is the shared front half of the
/// inspection and ThinLTO codegen verbs after the ordinary build path moved to the pipeline.
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


/// Compile `path` **per unit** (walk the import DAG bottom-up, one object per unit under the
/// separate-compilation visibility model) and link the N objects into `exe`. The one build path for
/// `build`/`run`/`size` after the M15 S2b flip. Objects stage in a process-private directory (named
/// by a per-unit index, not the `.`-containing module path); capability libraries are unioned
/// deterministically first-seen across units; the executable is published to `exe` by same-directory
/// atomic rename. Returns the failing `ExitCode` (diagnostics already printed) on any error.
#[allow(clippy::too_many_arguments)]
fn build_per_unit_to(path: &str, exe: &Path, target: BuildTarget, profile: Profile, rt_lto: bool, thin_lto: bool, pgo: &align_driver::PgoMode, jobs: usize, cache_stats: bool) -> Result<(), ExitCode> {
    // The persistent unit-frontend cache is consulted only on the ordinary per-unit path. A
    // `--thin-lto` build needs every unit's MIR for its prelink phase, so reuse there would
    // rehydrate all of them and buy nothing; it takes the unchanged `build_per_unit` route below.
    if !thin_lto {
        return build_package_to(path, exe, target, profile, rt_lto, pgo, jobs, cache_stats, UnitReuse::Allowed);
    }
    let walk = walk_or_report(path).ok_or(ExitCode::FAILURE)?;
    let object_stage = align_driver::ArtifactStage::temp("align-per-unit-obj").map_err(|e| {
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
            render_thin_cache_stats(&outcomes, cache.codegen_is_enabled());
        }
        let link_libs = link_lib_union(walk.units.iter().map(|u| u.mir.link_libs.as_slice()));
        return finish_link(&link_libs, &obj_paths, exe, profile, &target, &align_driver::PgoMode::Off);
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
        render_cache_stats(&build.outcomes, cache.codegen_is_enabled());
    }
    // One aggregated Align-voice `--pgo-use` report over the units that actually ran (cache MISSES), then
    // proceed — a mismatched profile is a PERFORMANCE concern, never a correctness one (clang parity), so
    // it is a WARNING, never an abort. A `matched == 0` build (the profile applied to NOTHING) gets a
    // prominent "is this the right profile?" line; a partial match rides the per-unit staleness warnings.
    // Hard fails stay at the reliable layer (missing/bad-magic profdata; an Error-severity libLLVM
    // diagnostic), handled before/inside codegen. An all-hit build ran no LLVM, so has a `0/0` tally and no
    // warnings: any staleness was reported when each object was first built and is intrinsic to the bytes.
    report_pgo_use(pgo, &build);
    let link_libs = link_lib_union(walk.units.iter().map(|u| u.mir.link_libs.as_slice()));
    finish_link(&link_libs, &obj_paths, exe, profile, &target, pgo)
}

/// The aggregated `--pgo-use` report over the units that actually ran (cache MISSES). A mismatched
/// profile is a PERFORMANCE concern, never a correctness one (clang parity), so it is a WARNING,
/// never an abort. Hard fails stay at the reliable layer: a missing/bad-magic profdata, and an
/// Error-severity libLLVM diagnostic, both handled before/inside codegen. An all-hit build ran no
/// LLVM, so it has a `0/0` tally and no warnings — any staleness was reported when each object was
/// first built and is intrinsic to the cached bytes.
fn report_pgo_use(pgo: &align_driver::PgoMode, build: &align_driver::UnitCodegen) {
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
}

/// The ordinary (non-ThinLTO) `build`/`run`/`size` path, through the persistent unit-frontend cache.
///
/// `reuse` is `Allowed` on the first attempt. If a cached unit disagrees with recomputation, the
/// entry is unlinked and this runs ONCE more with `Forbidden` and a fresh `SourceMap`: the whole
/// package is rebuilt, not just that unit, because its dependents were checked against a summary
/// that has just been shown untrustworthy. A `Forbidden` build never rehydrates, so the retry cannot
/// loop.
#[allow(clippy::too_many_arguments)]
fn build_package_to(path: &str, exe: &Path, target: BuildTarget, profile: Profile, rt_lto: bool, pgo: &align_driver::PgoMode, jobs: usize, cache_stats: bool, reuse: UnitReuse) -> Result<(), ExitCode> {
    // Only the retry pass forbids reuse here, and it re-derives what the first pass already
    // printed. (`UnitReuse` is `#[non_exhaustive]`, so this is a first-attempt-vs-retry test rather
    // than an exhaustive match.)
    let echo = if reuse == UnitReuse::Allowed { DiagnosticEcho::All } else { DiagnosticEcho::ErrorsOnly };
    let src = read(path).ok_or(ExitCode::FAILURE)?;
    let mut source_map = SourceMap::new();
    let cache = CacheContext::from_env();
    let cache_enabled = cache.codegen_is_enabled();
    let result = align_driver::build_package_pipelined(
        &mut source_map,
        path,
        &src,
        cache,
        reuse,
        &target,
        profile,
        rt_lto,
        jobs,
        pgo,
    );
    let render = |diags: &align_diag::Diagnostics| {
        let echo_now = match echo {
            DiagnosticEcho::All => !diags.is_empty(),
            // The retry re-derives first-attempt warnings. New errors are never suppressed.
            DiagnosticEcho::ErrorsOnly => diags.has_errors(),
        };
        if echo_now {
            eprint!("{}", format_diagnostics(&source_map, diags));
        }
    };
    let build = match result {
        align_driver::PipelinedPackageBuild::FrontendFailed { diags } => {
            render(&diags);
            return Err(ExitCode::FAILURE);
        }
        align_driver::PipelinedPackageBuild::Complete(build) => {
            render(&build.diags);
            build
        }
        // The one recoverable failure, matched on its SHAPE. The entry is already unlinked, so one
        // reuse-forbidden rebuild both succeeds and leaves the cache clean; a forbidden build never
        // rehydrates, so this cannot recur. The attempt's diagnostics are rendered against its own
        // still-live SourceMap before a retry creates a fresh one.
        align_driver::PipelinedPackageBuild::CodegenFailed {
            diags,
            error: align_driver::PackageCodegenError::StaleCacheEntry { unit, failure },
        } if reuse == UnitReuse::Allowed => {
            render(&diags);
            eprintln!(
                "alignc: cached unit `{unit}`: {failure}; rebuilding this package without cache reuse"
            );
            return build_package_to(path, exe, target, profile, rt_lto, pgo, jobs, cache_stats, UnitReuse::Forbidden);
        }
        align_driver::PipelinedPackageBuild::CodegenFailed { diags, error } => {
            render(&diags);
            eprintln!("alignc: {error}");
            return Err(ExitCode::FAILURE);
        }
    };
    if cache_stats {
        render_frontend_cache_stats(&build.units);
        render_cache_stats(&build.codegen.outcomes, cache_enabled);
    }
    report_pgo_use(pgo, &build.codegen);
    let link_libs = link_lib_union(build.units.iter().map(|u| u.link_libs.as_slice()));
    let obj_paths: Vec<PathBuf> = build.units.iter().map(|unit| unit.object().to_path_buf()).collect();
    finish_link(&link_libs, &obj_paths, exe, profile, &target, pgo)
}

/// Link the per-unit objects into `exe`: the deterministic capability-library union (first-seen in
/// DAG order) + link + atomic-rename publish. Shared by the normal cached path and the `--thin-lto`
/// path (the objects differ; the link step is identical).
fn finish_link(link_libs: &[String], obj_paths: &[PathBuf], exe: &Path, profile: Profile, target: &BuildTarget, pgo: &align_driver::PgoMode) -> Result<(), ExitCode> {

    let parent = exe.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let publish_stage = align_driver::ArtifactStage::in_dir(parent, "align-publish").map_err(|e| {
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
        align_driver::link_objects_instrumented(&obj_refs, &staged_exe, link_libs, profile, &profile_rt)
    } else {
        link_objects(&obj_refs, &staged_exe, link_libs, profile)
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

/// The `--cache-stats` FRONTEND block, printed before the unchanged codegen block. One line per
/// unit that actually consulted the stage, then its own summary. A unit whose `frontend` is `None`
/// declined the stage (cache disabled, reuse forbidden, descriptor-owning) and is counted in
/// neither hits nor misses — the same accounting rule the in-process memo uses. When no unit
/// consulted it, the block is absent entirely, so a `--thin-lto` or cache-off build prints exactly
/// what it printed before.
fn render_frontend_cache_stats(units: &[align_driver::PipelinedBuiltUnit]) {
    let outcomes: Vec<&align_driver::CacheOutcome> =
        units.iter().filter_map(|unit| unit.frontend.as_ref()).collect();
    if outcomes.is_empty() {
        return;
    }
    let (mut hits, mut misses) = (0usize, 0usize);
    for outcome in &outcomes {
        if outcome.hit {
            hits += 1;
            eprintln!("alignc: cache: {} frontend hit", outcome.unit);
        } else {
            misses += 1;
            let reason = outcome.miss_reason.map(|r| r.reason()).unwrap_or("miss");
            eprintln!("alignc: cache: {} frontend miss ({reason})", outcome.unit);
        }
    }
    eprintln!("alignc: cache: {} frontend: {hits} hit, {misses} miss", outcomes.len());
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
    let stage = match align_driver::ArtifactStage::temp("align-run") {
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
