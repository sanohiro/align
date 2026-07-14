//! Driver: connects the stages (`docs/impl/01-pipeline.md`).
//!
//! Exposes the `source.align` -> lexer -> parser -> sema -> MIR -> (codegen)
//! pipeline as library functions. Both the `alignc` binary (`main.rs`) and the
//! integration tests call this.

use align_diag::{Diagnostics, Severity};
use align_span::SourceMap;
pub use align_codegen_llvm::{
    target_object_format, BuildTarget, DebugInfo, ObjectFormat, Profile,
};
/// The lowered MIR program type (re-exported so callers can name it without depending on
/// `align_mir` directly).
pub use align_mir::Program as MirProgram;
/// M15 interface-summary types (re-exported so callers can name the [`check_per_unit`] result without
/// depending on `align_interface` directly).
pub use align_interface::{Hash128, InterfaceSummary};

pub mod explain;

/// Result of running the pipeline through sema.
pub struct Checked {
    pub hir: align_sema::Program,
    pub diags: Diagnostics,
}

/// lexer -> parser -> sema for the entry file plus its transitively-imported **user** modules
/// (multi-file, slice B1). User modules resolve by filename convention: `import geom` →
/// `<entry-dir>/geom.align`, which must declare `module geom`. Builtin imports (`core.*`/`std.*`)
/// are not files. Diagnostics are collected into `Checked.diags`.
/// A parsed source module (kept alive so `align_sema::Module` borrows into its `ast` are valid).
struct LoadedUnit {
    path: String,
    ast: align_ast::File,
    is_entry: bool,
    /// The module's full source text — retained for the M15 interface summary (generic template
    /// bodies + const values are recorded as source slices, and `impl_hash` is over these bytes).
    src: String,
}

// A user-module import is one whose first segment is neither `core` nor `std` (builtins).
fn user_import(p: &align_ast::Path) -> bool {
    p.segments.first().is_some_and(|s| s.name != "core" && s.name != "std")
}

/// lexer -> parser for the entry file plus its transitively-imported **user** modules, plus the
/// cyclic-import (DAG) check. The shared front half of [`check`] and [`build_interface_summaries`];
/// behavior-identical to the former inline loader.
fn load_units(source_map: &mut SourceMap, name: &str, src: &str, diags: &mut Diagnostics) -> Vec<LoadedUnit> {
    let entry_dir = std::path::Path::new(name).parent().map(|p| p.to_path_buf());

    // The entry module's own name is its `module` decl, or `main` by default.
    let entry_tokens = align_lexer::tokenize(source_map.add_file(name, src), src, diags);
    let entry_ast = align_parser::parse_file(entry_tokens, diags);
    let entry_path = entry_ast
        .module
        .as_ref()
        .and_then(|m| m.segments.last())
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "main".to_string());

    let mut loaded =
        vec![LoadedUnit { path: entry_path.clone(), ast: entry_ast, is_entry: true, src: src.to_string() }];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::from([entry_path.clone()]);

    // Edges of the module import graph (`importer path` -> `(imported modpath, import span)`),
    // collected for every `import` seen below regardless of the `seen` dedup — the dedup exists
    // only to avoid loading a shared module twice (the diamond case: `main` imports `b` and `c`,
    // both import `d`), not to license cycles. [`detect_import_cycles`] walks this graph
    // afterwards to tell that legal reconvergence apart from an actual cycle.
    let mut edges: std::collections::HashMap<String, Vec<(String, align_span::Span)>> =
        std::collections::HashMap::new();

    // Breadth-first over user-module imports, resolving each to `<entry-dir>/<name>.align`.
    let mut i = 0;
    while i < loaded.len() {
        let cur_path = loaded[i].path.clone();
        let imports: Vec<align_ast::Path> =
            loaded[i].ast.imports.iter().filter(|p| user_import(p)).cloned().collect();
        i += 1;
        for imp in imports {
            // The dotted module path (`util.math`) and the matching file path under the entry
            // directory (`util/math.align`): each segment is a directory, the last names the file.
            let modpath = imp.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".");
            edges.entry(cur_path.clone()).or_default().push((modpath.clone(), imp.span));
            if !seen.insert(modpath.clone()) {
                continue; // already loaded (shared / cyclic import)
            }
            let Some(dir) = &entry_dir else {
                diags.error(format!("cannot resolve `import {modpath}`: the entry file has no directory"), imp.span);
                continue;
            };
            let mut file_path = dir.clone();
            for seg in &imp.segments {
                file_path.push(&seg.name);
            }
            file_path.set_extension("align");
            let msrc = match std::fs::read_to_string(&file_path) {
                Ok(s) => s,
                Err(e) => {
                    diags.error(format!("cannot find module `{modpath}` (expected {}): {e}", file_path.display()), imp.span);
                    continue;
                }
            };
            let fid = source_map.add_file(file_path.display().to_string(), msrc.clone());
            let toks = align_lexer::tokenize(fid, &msrc, diags);
            let mast = align_parser::parse_file(toks, diags);
            // The file must declare the full `module util.math` (path ↔ filename agreement).
            let declared = mast.module.as_ref().map(|m| m.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."));
            if declared.as_deref() != Some(modpath.as_str()) {
                diags.error(
                    format!("module file `{}` must declare `module {modpath}` (found {})", file_path.display(),
                        declared.map(|d| format!("`module {d}`")).unwrap_or_else(|| "no module declaration".to_string())),
                    imp.span,
                );
            }
            loaded.push(LoadedUnit { path: modpath, ast: mast, is_entry: false, src: msrc });
        }
    }

    // The unit import graph must be a DAG (`draft.md` §17, M15 S0): a cycle of `import`s — direct,
    // transitive, or a module importing itself — is a compile error, not something the `seen`
    // dedup above should silently absorb. Sema still runs afterwards (this stage "continues as far
    // as possible on failure, accumulating diagnostics" — `align_diag`'s contract): a cyclic import
    // graph does not itself confuse per-module sema, and running it may surface further, genuinely
    // separate errors.
    detect_import_cycles(&entry_path, &edges, diags);

    loaded
}

pub fn check(source_map: &mut SourceMap, name: &str, src: &str) -> Checked {
    let mut diags = Diagnostics::new();
    let loaded = load_units(source_map, name, src, &mut diags);
    let modules: Vec<align_sema::Module> = loaded
        .iter()
        .map(|l| align_sema::Module { path: l.path.clone(), file: &l.ast, is_entry: l.is_entry, interface_only: false })
        .collect();
    let hir = align_sema::check_program(&modules, &mut diags);

    Checked { hir, diags }
}

/// M15 S1a producer entry point: run the frontend for the entry file + its transitively-imported
/// user modules, and — if it type-checks cleanly — build one [`align_interface::InterfaceSummary`]
/// per unit (with its interface / impl hashes). Additive: it does not touch the build/run path. On
/// any frontend error, returns no summaries plus the diagnostics (a summary of an ill-typed program
/// would be meaningless).
pub fn build_interface_summaries(
    source_map: &mut SourceMap,
    name: &str,
    src: &str,
) -> (Vec<align_interface::InterfaceSummary>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let loaded = load_units(source_map, name, src, &mut diags);
    let modules: Vec<align_sema::Module> = loaded
        .iter()
        .map(|l| align_sema::Module { path: l.path.clone(), file: &l.ast, is_entry: l.is_entry, interface_only: false })
        .collect();
    let hir = align_sema::check_program(&modules, &mut diags);
    if diags.has_errors() {
        return (Vec::new(), diags);
    }
    let mir = lower_to_mir(&hir);
    let sources: std::collections::HashMap<String, String> =
        loaded.iter().map(|l| (l.path.clone(), l.src.clone())).collect();
    let summaries = align_interface::build_summaries(&modules, &hir, &mir, &sources);
    (summaries, diags)
}

/// M15 S1b per-unit check result. `check_per_unit` walks the import DAG bottom-up, checking each
/// unit against the already-checked *interface summaries* of its (transitive) imports — never their
/// ASTs — and re-deriving each unit's own summary from that per-unit check.
pub struct PerUnitCheck {
    /// One interface summary per unit that checked cleanly, in bottom-up (dependency-first) order.
    /// A unit whose body fails to check contributes no summary (a summary of an ill-typed unit is
    /// meaningless), so dependents of a broken unit see it as an absent dependency.
    pub summaries: Vec<align_interface::InterfaceSummary>,
    /// For each unit (by module path, bottom-up), the TRANSITIVE set of imported units it depended
    /// on, each paired with that dependency's `interface_hash`. This is the S3 incremental-cache key
    /// input: a unit must be re-checked when any entry here changes. Foreign type references are
    /// by-name in the canonical surface, so the dependency is transitive, not just direct.
    pub dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)>,
    /// The union of every unit's per-unit diagnostics (each unit's diagnostics are emitted once, when
    /// that unit is the unit-under-check; interface-only dependencies emit none).
    pub diags: Diagnostics,
}

/// M15 S1b: check every unit **per-unit**, each against only its own AST plus the interface summaries
/// of its (transitively-closed) imports — the literal reading of `draft.md` §17 ("each module is
/// checked against the already-checked interfaces of its imports"). This is an ADDITIVE capability
/// proving the separate-compilation seam; it does not replace the whole-program [`check`] build path
/// (S2 flips codegen). Units are processed bottom-up over the import DAG (S0 guarantees acyclicity);
/// each dependency's public surface is rendered back to source and re-parsed into an interface-only
/// module (one resolution path — the existing sema passes), and cross-unit effect bits are seeded
/// fail-closed.
pub fn check_per_unit(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitCheck {
    use std::collections::HashMap;
    let mut diags = Diagnostics::new();
    let loaded = load_units(source_map, name, src, &mut diags);

    let by_path: HashMap<&str, &LoadedUnit> = loaded.iter().map(|l| (l.path.as_str(), l)).collect();
    // Each unit's direct user-module dependencies, in import-declaration order (deterministic).
    let direct_deps: HashMap<String, Vec<String>> = loaded
        .iter()
        .map(|l| {
            let deps: Vec<String> = l
                .ast
                .imports
                .iter()
                .filter(|p| user_import(p))
                .map(|p| p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."))
                .filter(|d| by_path.contains_key(d.as_str()))
                .collect();
            (l.path.clone(), deps)
        })
        .collect();

    // Transitive dependency closure of `start` (excluding `start`), deterministic (import order).
    fn transitive(start: &str, direct: &HashMap<String, Vec<String>>) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut order = Vec::new();
        fn go(
            node: &str,
            direct: &HashMap<String, Vec<String>>,
            seen: &mut std::collections::HashSet<String>,
            order: &mut Vec<String>,
        ) {
            if let Some(deps) = direct.get(node) {
                for d in deps {
                    if seen.insert(d.clone()) {
                        go(d, direct, seen, order);
                        order.push(d.clone());
                    }
                }
            }
        }
        go(start, direct, &mut seen, &mut order);
        order
    }

    // Bottom-up (dependency-first) order: DFS post-order from the entry unit. All loaded units are
    // reachable from the entry (they were loaded by following its imports). A `visited` guard makes
    // this terminate even if S0's cycle check already flagged a cycle (a best-effort order then).
    let entry = loaded.iter().find(|l| l.is_entry).map(|l| l.path.clone()).unwrap_or_default();
    let mut order: Vec<String> = Vec::new();
    {
        let mut visited = std::collections::HashSet::new();
        fn post(
            node: &str,
            direct: &HashMap<String, Vec<String>>,
            visited: &mut std::collections::HashSet<String>,
            order: &mut Vec<String>,
        ) {
            if !visited.insert(node.to_string()) {
                return;
            }
            if let Some(deps) = direct.get(node) {
                for d in deps {
                    post(d, direct, visited, order);
                }
            }
            order.push(node.to_string());
        }
        post(&entry, &direct_deps, &mut visited, &mut order);
        // Include any unit not reachable from the entry (defensive; normally none).
        for l in &loaded {
            if !visited.contains(&l.path) {
                post(&l.path, &direct_deps, &mut visited, &mut order);
            }
        }
    }

    let mut summaries: HashMap<String, align_interface::InterfaceSummary> = HashMap::new();
    let mut dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)> = Vec::new();
    // Cache of each dependency's synthesized interface AST, keyed by module path. Rendered and
    // parsed exactly once per dependency (not once per importer): `summary_to_source` is called
    // with the DEP'S OWN transitive closure, never the importer's, so the rendered source (and
    // therefore the parsed AST) is importer-independent and safe to share across every unit that
    // imports it. Without this, the bottom-up walk below would re-render and re-parse every
    // transitive dependency's summary once per importer — O(N^2) in the DAG's fan-in.
    let mut interface_ast_cache: HashMap<String, align_ast::File> = HashMap::new();

    for unit_path in &order {
        let Some(u) = by_path.get(unit_path.as_str()).copied() else { continue };
        let tdeps = transitive(unit_path, &direct_deps);

        // The S3 cache key input: this unit's transitive dependency interface hashes.
        let hset: Vec<(String, align_interface::Hash128)> = tdeps
            .iter()
            .filter_map(|d| summaries.get(d).map(|s| (d.clone(), s.interface_hash)))
            .collect();
        dep_interface_hashes.push((unit_path.clone(), hset));

        // Reconstruct each transitive dependency as an interface-only module from its summary,
        // reusing (or populating) `interface_ast_cache` so each dependency is rendered and parsed
        // exactly once across the whole bottom-up walk, not once per importer.
        let mut external_effects: HashMap<String, align_sema::FnEffect> = HashMap::new();
        for d in &tdeps {
            let Some(dep_summary) = summaries.get(d) else { continue };
            if !interface_ast_cache.contains_key(d) {
                // Render using `d`'s OWN transitive closure (never the importer's `tdeps`): that
                // is what makes the rendered source, and therefore the parsed AST, independent of
                // which importer triggered the parse — and so safe to cache and share.
                let d_tdeps = transitive(d, &direct_deps);
                let d_tdep_refs: Vec<&str> = d_tdeps.iter().map(|s| s.as_str()).collect();
                let source = align_interface::summary_to_source(dep_summary, &d_tdep_refs);
                // Parse the synthesized surface with the real parser (one resolution path). Synthesized
                // source is compiler-internal and always well-formed; its parse diagnostics are discarded.
                let mut sink = Diagnostics::new();
                let fid = source_map.add_file(format!("<interface:{d}>"), source.clone());
                let toks = align_lexer::tokenize(fid, &source, &mut sink);
                let ast = align_parser::parse_file(toks, &mut sink);
                interface_ast_cache.insert(d.clone(), ast);
            }
            external_effects.extend(align_interface::summary_effects(dep_summary, false));
        }

        let mut modules: Vec<align_sema::Module> = tdeps
            .iter()
            .filter_map(|d| {
                interface_ast_cache.get(d).map(|ast| align_sema::Module {
                    path: d.clone(),
                    file: ast,
                    is_entry: false,
                    interface_only: true,
                })
            })
            .collect();
        modules.push(align_sema::Module {
            path: u.path.clone(),
            file: &u.ast,
            is_entry: u.is_entry,
            interface_only: false,
        });

        let mut u_diags = Diagnostics::new();
        let program = align_sema::check_program_with_effects(&modules, &external_effects, &mut u_diags);
        let had_errors = u_diags.has_errors();
        for d in u_diags.iter() {
            diags.push(d.clone());
        }

        if !had_errors {
            // Re-derive THIS unit's own summary from its per-unit check (bottom-up: dependencies are
            // already summarized). Only the unit's real module is passed, so exactly one summary is
            // built. Its `interface_hash` folds cross-unit effect bits (`external_effects`).
            let unit_module = [align_sema::Module {
                path: u.path.clone(),
                file: &u.ast,
                is_entry: u.is_entry,
                interface_only: false,
            }];
            let mir = lower_to_mir(&program);
            let sources: HashMap<String, String> = HashMap::from([(u.path.clone(), u.src.clone())]);
            let mut built = align_interface::build_summaries_with_effects(
                &unit_module,
                &program,
                &mir,
                &sources,
                &external_effects,
            );
            if let Some(s) = built.pop() {
                summaries.insert(u.path.clone(), s);
            }
        }
    }

    // Emit summaries in bottom-up order (deterministic, dependency-first).
    let summaries: Vec<align_interface::InterfaceSummary> =
        order.iter().filter_map(|p| summaries.remove(p)).collect();

    PerUnitCheck { summaries, dep_interface_hashes, diags }
}

/// Reject a cyclic module import graph (`check`'s `edges` map: importer path -> `(imported
/// modpath, import span)`), M15 S0 — `draft.md` §17 requires the import graph to be a DAG. A
/// standard depth-first white/grey/black walk from `start` (the entry module): white = unvisited,
/// grey = open on the current DFS path, black = fully explored. A White target recurses; a Black
/// target is a **diamond** (already fully explored via an earlier sibling branch — `b` and `c` both
/// importing `d` is legal reconvergence, not a cycle) and is skipped; a Grey target means the edge
/// closes a cycle back to a module still open on the current path, direct (`a` -> `b` -> `a`),
/// transitive, or a self-import (`a` -> `a`) — reported once, at the closing edge's span, and the
/// walk stops (no cascading cyclic-import diagnostics for the same cycle).
fn detect_import_cycles(
    start: &str,
    edges: &std::collections::HashMap<String, Vec<(String, align_span::Span)>>,
    diags: &mut Diagnostics,
) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Grey,
        Black,
    }

    fn visit<'a>(
        node: &'a str,
        edges: &'a std::collections::HashMap<String, Vec<(String, align_span::Span)>>,
        color: &mut std::collections::HashMap<&'a str, Color>,
        path: &mut Vec<&'a str>,
        diags: &mut Diagnostics,
    ) -> bool {
        color.insert(node, Color::Grey);
        path.push(node);
        if let Some(outs) = edges.get(node) {
            for (target, span) in outs {
                let target = target.as_str();
                match color.get(target).copied().unwrap_or(Color::White) {
                    Color::White => {
                        if visit(target, edges, color, path, diags) {
                            return true;
                        }
                    }
                    Color::Grey => {
                        // `target` is still open on the current DFS path: this edge is the back
                        // edge that closes the cycle. Render the path from `target`'s position on
                        // the stack through the current node, then back to `target`.
                        let start_ix = path.iter().position(|&p| p == target).unwrap_or(0);
                        let mut cycle = path[start_ix..].to_vec();
                        cycle.push(target);
                        diags.error(
                            format!(
                                "cyclic import: {} (the module import graph must be a DAG; merge \
                                 the modules or extract the shared part into a module both import)",
                                cycle.join(" -> ")
                            ),
                            *span,
                        );
                        return true;
                    }
                    Color::Black => {} // fully explored on an earlier branch: a diamond, not a cycle
                }
            }
        }
        path.pop();
        color.insert(node, Color::Black);
        false
    }

    let mut color = std::collections::HashMap::new();
    let mut path = Vec::new();
    visit(start, edges, &mut color, &mut path, diags);
}

/// Lower the sema-checked HIR down to MIR.
pub fn lower_to_mir(hir: &align_sema::Program) -> align_mir::Program {
    align_mir::lower_program(hir)
}

/// Lower to MIR with source locations (each statement records the line/col it came from), for
/// `explain-opt` / debug-info emission. Identical to [`lower_to_mir`] but with populated
/// `stmt_lines`.
pub fn lower_to_mir_located(hir: &align_sema::Program, source_map: &SourceMap) -> align_mir::Program {
    align_mir::lower_program_located(hir, source_map)
}

/// Whether the LLVM backend is available (codegen is wired up).
pub fn backend_available() -> bool {
    align_codegen_llvm::is_available()
}

/// Compile `mir` with debug locations, run `-O2`, and return LLVM's raw optimization-remark strings
/// (`"<file>:<line>:<col>: <message>"`). Process-global side effect — see
/// [`align_codegen_llvm::collect_opt_remarks`]. Used only by `explain-opt`.
pub fn collect_opt_remarks(
    mir: &align_mir::Program,
    target: BuildTarget,
    debug: &DebugInfo,
) -> Result<Vec<String>, String> {
    align_codegen_llvm::collect_opt_remarks(mir, &target, debug).map_err(|e| e.to_string())
}

/// Write MIR out to an object file (codegen). `target` selects the CPU baseline (portable default
/// vs. host-`native`); `profile` selects the middle-end pipeline (`default<O0|O2|O3|Os|Oz>`).
/// `exports` are the explicit export roots (`emit-obj --export`, M13 Codex-audit item 1): the
/// program-function names (matched against source-level `Function::name`, validate with
/// [`unknown_exports`] first) that keep `external` linkage instead of the default whole-program
/// `internal`. Empty for every caller except `emit-obj`/`emit-llvm`.
/// The fast-path string-primitive bitcode (`build.rs` → `str_prims.bc`), baked into `alignc`. Passed
/// to codegen as the `--rt-lto` artifact when `rt_lto` is set; parsing/linking it is codegen's job
/// (`link_in_rt_lto`), with a fail-loud fallback to the runtime staticlib on an unparseable artifact.
/// Baking dissolves the staleness question — the same `cargo build` regenerates it (M14 Slice 2).
const RT_LTO_BITCODE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/str_prims.bc"));

/// The baked `--rt-lto` bitcode when `rt_lto` is on, else `None` (the byte-identical flag-off path).
fn rt_lto_bytes(rt_lto: bool) -> Option<&'static [u8]> {
    rt_lto.then_some(RT_LTO_BITCODE)
}

/// The baked `--rt-lto` fast-path string-primitive bitcode (`build.rs` → `str_prims.bc`). Exposed
/// read-only for the M14 Slice-2 artifact gates (the symbol-set pin: `llvm-nm` must show the guarded
/// four as the only defined `align_rt_*` symbols) and any tooling that inspects the artifact.
pub fn rt_lto_bitcode() -> &'static [u8] {
    RT_LTO_BITCODE
}

pub fn emit_object_file(mir: &align_mir::Program, obj: &std::path::Path, target: BuildTarget, profile: Profile, exports: &[String], rt_lto: bool) -> Result<(), String> {
    align_codegen_llvm::emit_object(mir, obj, &target, profile, exports, rt_lto_bytes(rt_lto)).map_err(|e| e.to_string())
}

/// MIR to LLVM IR text (`alignc emit-llvm`). `optimized` picks the lens: `false` (`--stage raw`)
/// prints what codegen emitted; `true` (`--stage optimized`) runs the `-O2` pipeline first, so the
/// output shows what LLVM actually did (inlined, fused, vectorized). `exports` is the same
/// export-roots list as [`emit_object_file`].
pub fn emit_llvm_ir(mir: &align_mir::Program, target: BuildTarget, optimized: bool, exports: &[String], rt_lto: bool) -> Result<String, String> {
    align_codegen_llvm::emit_llvm_ir(mir, &target, optimized, exports, rt_lto_bytes(rt_lto)).map_err(|e| e.to_string())
}

/// The names in `exports` that do not match any function in `mir` (by [`align_mir::Function::name`]).
/// Empty ⇒ every requested export root resolves. The fail-closed seam for `--export <name>`: an
/// unknown name must be a hard, listed error (`alignc: unknown export(s): …`), never a silent no-op
/// (a typo'd export name would otherwise compile a wrong object with no diagnostic at all).
pub fn unknown_exports<'a>(mir: &align_mir::Program, exports: &'a [String]) -> Vec<&'a str> {
    exports
        .iter()
        .filter(|name| !mir.fns.iter().any(|f| &f.name == *name))
        .map(String::as_str)
        .collect()
}

/// Link an object into an executable. Uses the system C compiler (`cc`); crt0 calls
/// the generated `main` as the entry point (`docs/impl/01-pipeline.md`: driver links).
///
/// The thin runtime (`libalign_runtime.a`, e.g. the builtin `print`) is linked in too. Being a
/// Rust staticlib, it needs the usual std support libraries (`pthread`/`dl`/`m` on ELF; on Mach-O
/// they are libSystem re-exports — see [`support_libs`]).
pub fn link_executable(obj: &std::path::Path, exe: &std::path::Path, link_libs: &[String], profile: Profile) -> Result<(), String> {
    link_objects(&[obj], exe, link_libs, profile)
}

/// Link one or more object files (plus the Align runtime and the always-linked C libraries) into an
/// executable. The single-object [`link_executable`] is the common case; multiple objects are used
/// by the FFI tests that link an Align object against a compiled C-helper object (a by-value struct
/// callee), and by any future multi-translation-unit build.
pub fn link_objects(objs: &[&std::path::Path], exe: &std::path::Path, link_libs: &[String], profile: Profile) -> Result<(), String> {
    let format = target_object_format()?;
    let runtime = runtime_archive()?;
    let mut cmd = std::process::Command::new("cc");
    for obj in objs {
        cmd.arg(obj);
    }
    cmd.arg(&runtime)
        .arg("-o")
        .arg(exe)
        // Link hygiene (M13 Slice 2), spelled per object format by `hygiene_flags`. Dead-code
        // removal (ELF `--gc-sections` / Mach-O `-dead_strip`) drops every unreferenced input
        // section from the final image; combined with the runtime's per-function sections (Rust's
        // default) this garbage-collects the `std.compress`/`std.crypto`/`std.http` code a program
        // does not use, eliminating its `libz`/`libzstd`/`libcrypto`/`libssl` references so those
        // libraries are not needed at all. Unused-dylib removal (ELF `--as-needed` / Mach-O
        // `-dead_strip_dylibs`) then records a dependency (`DT_NEEDED` / `LC_LOAD_DYLIB`) only for
        // libraries that actually satisfy a surviving reference. Both are correctness-neutral
        // hygiene, kept for EVERY profile (M13 Slice 4) — even `dev`: the potential link-speed
        // saving of dropping dead-code removal is not worth a second link-flag path, and a `dev`
        // binary that silently links dead `libssl` etc. would be a surprising difference from
        // `release`.
        .args(hygiene_flags(format));
    // Per-profile strip (M13 Slice 4). The size profiles (`small`/`tiny`) drop the whole symbol
    // table; the speed profiles (`dev`/`release`/`fast`) keep symbols so a crash backtrace / `perf`
    // stays useful. The strip *decision* is owned by `Profile::strip` alone; only the *spelling*
    // is per-format: ELF strips in the link (`-Wl,--strip-all`), Mach-O has no ld64 equivalent and
    // runs the external `strip` after a successful link (below).
    if profile.strip() && format == ObjectFormat::Elf {
        cmd.arg("-Wl,--strip-all");
    }
    // The always-linked support libraries, per format (`support_libs`): on ELF,
    // `libpthread`/`libdl`/`libm` are Rust-std support libraries the runtime *core* may reference
    // (threads, dlopen, math) independent of any Align feature — NOT capability-gated. On Mach-O
    // all three are libSystem re-exports, so the list is empty.
    cmd.args(support_libs(format));
    // Capability + user libraries. `libz`/`libzstd`/`libcrypto`/`libssl` are NO LONGER linked
    // unconditionally: they now arrive through `link_libs`, which MIR populates from the builtins a
    // program actually uses (`align_mir::Capability`) plus any `extern "C" link("name")` the user
    // declared (validated in sema). All go AFTER the objects/archive that reference them (`-l`
    // resolves left-to-right against preceding inputs). Each name is a single `-l<name>` argv (no
    // shell/flag injection). A program using no gated feature links none of z/zstd/crypto/ssl.
    for lib in link_libs {
        cmd.arg(format!("-l{lib}"));
    }
    let status = cmd
        .status()
        .map_err(|e| format!("cannot launch cc: {e}"))?;
    if !status.success() {
        return Err(link_failure_message(status.code(), link_libs));
    }
    // Mach-O strip: ld64 has no `--strip-all`, so the size profiles run the external `strip` on the
    // linked image. `strip` ships with the same Xcode CLT as the `cc`/`ld` above (the existing
    // implicit toolchain dependency), and it re-signs the stripped binary ad hoc, so the result
    // stays runnable. A launch failure or nonzero exit is a hard error, same as a failed link — the
    // profile's contract (all symbols removed) must never be broken silently.
    if profile.strip() && format == ObjectFormat::MachO {
        let strip_status = std::process::Command::new("strip")
            .arg(exe)
            .status()
            .map_err(|e| format!("cannot launch strip: {e}"))?;
        if !strip_status.success() {
            return Err(format!("strip failed (exit code {:?})", strip_status.code()));
        }
    }
    Ok(())
}

/// The link-hygiene flags for `format` (see the call site in [`link_objects`] for what they do).
/// A data table, same shape as `Profile::pipeline` — the *meaning* is format-independent, only the
/// spelling differs.
fn hygiene_flags(format: ObjectFormat) -> &'static [&'static str] {
    match format {
        // Dead-section removal + record only the shared libraries that resolve a reference.
        ObjectFormat::Elf => &["-Wl,--gc-sections", "-Wl,--as-needed"],
        ObjectFormat::MachO => &["-Wl,-dead_strip", "-Wl,-dead_strip_dylibs"],
    }
}

/// The always-linked support libraries for `format` (see the call site in [`link_objects`]).
/// Mach-O has none: `pthread`/`dl`/`m` are all libSystem re-exports there, so naming them is noise.
fn support_libs(format: ObjectFormat) -> &'static [&'static str] {
    match format {
        ObjectFormat::Elf => &["-lpthread", "-ldl", "-lm"],
        ObjectFormat::MachO => &[],
    }
}

/// The gated capability libraries (`align_mir::Capability`): the ones a system commonly does NOT
/// ship in the default linker search path, so a link failure involving them gets a `LIBRARY_PATH`
/// hint appended ([`link_failure_message`]).
const GATED_LIBS: [&str; 4] = ["z", "zstd", "crypto", "ssl"];

/// The link-failure error. When the failed link involved a gated capability library, append a note
/// about non-default library prefixes: those libraries often live outside the default search path
/// (e.g. Homebrew keg-only OpenSSL on macOS), and the fix is the standard `LIBRARY_PATH` mechanism.
/// The driver never injects search paths itself — what is linked, and from where, stays visible
/// (Nothing hidden).
fn link_failure_message(code: Option<i32>, link_libs: &[String]) -> String {
    let mut msg = format!("link failed (cc exit code {code:?})");
    if link_libs.iter().any(|l| GATED_LIBS.contains(&l.as_str())) {
        msg.push_str(
            "\nnote: libraries in a non-default prefix (e.g. Homebrew keg-only) are found via \
             LIBRARY_PATH, e.g. LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib",
        );
    }
    msg
}

/// Locate the LLVM binutils replacement `name` (`llvm-readobj`, `llvm-nm`) with the version
/// matching the LLVM this compiler is built against. Used by `alignc size` and the link-inspection
/// tests. Search order:
///
///  1. `$LLVM_SYS_221_PREFIX/bin/<name>` — the build-time LLVM prefix (compile-time env, the same
///     variable llvm-sys builds from), so the tool version always matches the linked LLVM. A stale
///     baked-in path (prefix moved since the build) falls through.
///  2. `<name>-22` on `PATH` (apt.llvm.org naming; the suffix is
///     [`align_codegen_llvm::LLVM_TOOL_VERSION`]).
///  3. Plain `<name>` on `PATH`.
///
/// `None` when nothing is found — callers degrade the affected report section to a note.
pub fn llvm_tool(name: &str) -> Option<std::path::PathBuf> {
    llvm_tool_in(option_env!("LLVM_SYS_221_PREFIX"), name)
}

/// [`llvm_tool`] with the LLVM prefix as a parameter (unit-testable without rebaking the
/// compile-time env).
fn llvm_tool_in(prefix: Option<&str>, name: &str) -> Option<std::path::PathBuf> {
    if let Some(prefix) = prefix {
        let p = std::path::Path::new(prefix).join("bin").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    let versioned = format!("{name}-{}", align_codegen_llvm::LLVM_TOOL_VERSION);
    for cand in [versioned.as_str(), name] {
        // Minimal PATH probe: does `<cand> --version` launch and exit 0?
        let found = std::process::Command::new(cand)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if found {
            return Some(std::path::PathBuf::from(cand));
        }
    }
    None
}

/// The in-tree `align_runtime` source directory, baked in at build time (relative to this
/// crate's manifest). Present only when `alignc` runs from inside the workspace; an installed
/// binary has no source tree, so the staleness check below simply no-ops there.
const RUNTIME_SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../align_runtime/src");

/// Locate `libalign_runtime.a`, built by `cargo build` alongside the `alignc` binary.
/// The integration tests run from `target/<profile>/deps/`, so the parent is checked too.
fn runtime_archive() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find current exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;
    for cand in [dir.join("libalign_runtime.a"), dir.join("../libalign_runtime.a")] {
        if cand.exists() {
            ensure_archive_fresh(&cand)?;
            return Ok(cand);
        }
    }
    Err(format!(
        "cannot find libalign_runtime.a near {}; run `cargo build` first",
        dir.display()
    ))
}

/// Fail loudly if `libalign_runtime.a` is older than the `align_runtime` source.
///
/// `align_driver` has no cargo dependency edge to the runtime *staticlib*, and a unit-test
/// build (`cargo test -p align_runtime`) recompiles only the test harness — neither refreshes
/// the `.a`. So editing the runtime and re-running the driver/tests without a full `cargo build`
/// would silently link a *stale* archive: wrong behavior and baffling test failures (this has
/// bitten development; see `open-questions.md`). Converting that into an actionable error is the
/// stable-toolchain fix (an artifact dependency, the clean edge, is still nightly-only).
///
/// No-ops when the source tree is absent (an installed `alignc`) or unreadable — it only ever
/// turns a definitely-stale link into an error, never blocks a legitimate one.
fn ensure_archive_fresh(archive: &std::path::Path) -> Result<(), String> {
    let src = std::path::Path::new(RUNTIME_SRC_DIR);
    if !src.is_dir() {
        return Ok(()); // installed binary: no source tree to compare against
    }
    let Ok(archive_mtime) = archive.metadata().and_then(|m| m.modified()) else {
        return Ok(()); // cannot stat the archive: do not block the build
    };
    if let Some(newest) = newest_rs_mtime(src)
        && newest > archive_mtime {
            return Err(format!(
                "libalign_runtime.a is stale: a source file under {} is newer than the archive \
                 {}.\nThe driver has no cargo edge to the runtime staticlib, so run `cargo build` \
                 to refresh it before linking.",
                src.display(),
                archive.display(),
            ));
        }
    Ok(())
}

/// Newest modification time among `*.rs` files under `dir` (recursive); `None` if there are
/// none or the tree is unreadable. Unreadable subdirectories are skipped, not fatal — the check
/// must never disable itself silently on a single bad entry.
fn newest_rs_mtime(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            // `file_type()` comes from the `read_dir` iterator with no extra `stat`, and (unlike
            // `path.is_dir()`) does not follow symlinks — so a symlinked dir is not traversed,
            // avoiding cycles / escaping the source tree. We `stat` only actual `.rs` files.
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() && entry.path().extension().is_some_and(|x| x == "rs")
                && let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
                    newest = Some(newest.map_or(t, |n| n.max(t)));
                }
        }
    }
    newest
}

/// Format diagnostics for humans (one per line, `file:line:col: severity: message`).
pub fn format_diagnostics(source_map: &SourceMap, diags: &Diagnostics) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for d in diags.iter() {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        if let Some(span) = d.span {
            let f = source_map.get(span.file);
            let (line, col) = f.line_col(span.lo);
            let _ = writeln!(out, "{}:{}:{}: {}: {}", f.name, line, col, sev, d.message);
        } else {
            let _ = writeln!(out, "{}: {}", sev, d.message);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_rs_mtime_scans_recursively_and_filters_extension() {
        // Unique temp dir (no Date/rand in-crate; pid + a stack address suffice here).
        let root = std::env::temp_dir().join(format!(
            "align-driver-mtime-{}-{:p}",
            std::process::id(),
            &0u8 as *const _
        ));
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).expect("create temp tree");

        // Empty (no `.rs`) → None.
        assert_eq!(newest_rs_mtime(&root), None, "no .rs files yet");

        // A non-`.rs` file is ignored.
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        assert_eq!(newest_rs_mtime(&root), None, ".txt is not counted");

        // `.rs` files at the top level and in a subdir are both found; the result is their max
        // mtime. Compare against an independent scan so the test does not depend on write timing.
        std::fs::write(root.join("a.rs"), b"fn a() {}").unwrap();
        std::fs::write(sub.join("b.rs"), b"fn b() {}").unwrap();
        let expect = [root.join("a.rs"), sub.join("b.rs")]
            .iter()
            .map(|p| p.metadata().unwrap().modified().unwrap())
            .max()
            .unwrap();
        assert_eq!(
            newest_rs_mtime(&root),
            Some(expect),
            "finds the newest .rs across the top level and the nested dir"
        );

        // A missing directory yields None (read_dir fails, skipped, not a panic).
        assert_eq!(newest_rs_mtime(&root.join("does-not-exist")), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hygiene_flags_are_pinned_per_format() {
        // The flag tables ARE the linker policy — pin the exact spellings so a drive-by edit
        // cannot silently change what every `alignc build` passes to the linker.
        assert_eq!(hygiene_flags(ObjectFormat::Elf), ["-Wl,--gc-sections", "-Wl,--as-needed"]);
        assert_eq!(hygiene_flags(ObjectFormat::MachO), ["-Wl,-dead_strip", "-Wl,-dead_strip_dylibs"]);
    }

    #[test]
    fn support_libs_are_pinned_per_format() {
        assert_eq!(support_libs(ObjectFormat::Elf), ["-lpthread", "-ldl", "-lm"]);
        assert_eq!(support_libs(ObjectFormat::MachO), [] as [&str; 0]);
    }

    #[test]
    fn link_failure_message_hints_library_path_only_for_gated_libs() {
        // No gated library involved → the plain error, no note.
        let plain = link_failure_message(Some(1), &["m".to_string()]);
        assert!(plain.starts_with("link failed"));
        assert!(!plain.contains("LIBRARY_PATH"), "no hint without a gated lib:\n{plain}");
        // A gated library (Homebrew keg-only class) → the LIBRARY_PATH hint is appended.
        let hinted = link_failure_message(Some(1), &["z".to_string(), "ssl".to_string()]);
        assert!(hinted.contains("LIBRARY_PATH"), "gated libs get the hint:\n{hinted}");
        assert!(hinted.contains("/opt/homebrew/opt/openssl@3/lib"), "hint shows an example path:\n{hinted}");
    }

    #[test]
    fn llvm_tool_discovery_order() {
        // 1. A prefix that contains `bin/<name>` wins outright (mere existence, no launch).
        let root = std::env::temp_dir().join(format!(
            "align-driver-llvmtool-{}-{:p}",
            std::process::id(),
            &0u8 as *const _
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("create temp prefix");
        std::fs::write(bin.join("llvm-sometool"), b"").unwrap();
        let prefix = root.to_string_lossy().into_owned();
        assert_eq!(
            llvm_tool_in(Some(&prefix), "llvm-sometool"),
            Some(bin.join("llvm-sometool")),
            "the build-time prefix hit is taken first"
        );

        // 2. A stale prefix (no such file) falls through to the PATH probe; a name that exists
        //    nowhere yields None (the caller degrades to a note).
        assert_eq!(llvm_tool_in(Some(&prefix), "llvm-definitely-not-a-tool"), None);
        assert_eq!(llvm_tool_in(None, "llvm-definitely-not-a-tool"), None);

        std::fs::remove_dir_all(&root).ok();
    }
}
