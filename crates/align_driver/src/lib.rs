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

pub mod cache;
pub mod explain;

pub use cache::{clear_cache, CacheContext, CacheLookup, CacheOutcome, CacheStage, CodegenKey, FirstDiff};

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
    /// The module's source file path on disk (the entry file's given `name`, or a resolved
    /// `<dir>/<seg>.align`). Carried so a per-unit consumer (`explain-opt`) can build that unit's own
    /// `DebugInfo` (its basename is what LLVM's remark strings — and thus the report — attribute to).
    file: String,
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

    let mut loaded = vec![LoadedUnit {
        path: entry_path.clone(),
        ast: entry_ast,
        is_entry: true,
        src: src.to_string(),
        file: name.to_string(),
    }];
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
            loaded.push(LoadedUnit {
                path: modpath,
                ast: mast,
                is_entry: false,
                src: msrc,
                file: file_path.display().to_string(),
            });
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
fn walk_per_unit(source_map: &mut SourceMap, name: &str, src: &str, located: bool) -> PerUnitWalk {
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
    // Per-unit compilation artifacts, keyed by unit path: (summary, per-unit MIR, is_entry). Populated
    // only for cleanly-checked units; assembled bottom-up into `PerUnitArtifact`s at the end.
    let mut mirs: HashMap<String, (align_interface::InterfaceSummary, MirProgram, bool)> = HashMap::new();
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
            // Per-unit MIR (S2): the unit's own fns + in-consumer monomorphs, with the
            // separate-compilation visibility bits set (`pub` fns external, imported callees carried
            // as external declares). The summary is byte-identical whether lowered per-unit or
            // whole-program (its `impl_hash` hashes MIR *fns*, its capabilities partition MIR fns —
            // neither sees the exportable bit or the declare list), so `check_per_unit`'s summaries
            // are unchanged by this while `build_per_unit` reuses the same MIR for codegen.
            let mir = if located {
                lower_to_mir_per_unit_located(&program, source_map)
            } else {
                lower_to_mir_per_unit(&program)
            };
            let sources: HashMap<String, String> = HashMap::from([(u.path.clone(), u.src.clone())]);
            let mut built = align_interface::build_summaries_with_effects(
                &unit_module,
                &program,
                &mir,
                &sources,
                &external_effects,
            );
            if let Some(s) = built.pop() {
                summaries.insert(u.path.clone(), s.clone());
                mirs.insert(u.path.clone(), (s, mir, u.is_entry));
            }
        }
    }

    // Assemble one artifact per cleanly-checked unit, in bottom-up (dependency-first) order. A unit
    // that failed to check contributes none (its errors are in `diags`). `dep_interface_hashes` stays
    // the FULL per-order list (an entry for every unit, clean or not) — that is the S1b dev-verb
    // contract `check_per_unit` returns unchanged; each clean unit's artifact carries its own copy.
    let dep_hashes_by_unit: HashMap<&str, &Vec<(String, align_interface::Hash128)>> =
        dep_interface_hashes.iter().map(|(u, h)| (u.as_str(), h)).collect();
    let units: Vec<PerUnitArtifact> = order
        .iter()
        .filter_map(|p| {
            let (summary, mir, is_entry) = mirs.remove(p)?;
            Some(PerUnitArtifact {
                unit: p.clone(),
                is_entry,
                mir,
                summary,
                dep_interface_hashes: dep_hashes_by_unit
                    .get(p.as_str())
                    .unwrap_or_else(|| panic!("missing dependency hashes for unit '{p}' — walk order must produce deps first"))
                    .to_vec(),
                file: by_path.get(p.as_str()).map(|u| u.file.clone()).unwrap_or_default(),
            })
        })
        .collect();

    let _ = summaries; // superseded by `units[*].summary`; retained above only for the walk's seeding.
    PerUnitWalk { units, dep_interface_hashes, diags }
}

/// One unit's per-unit compilation artifact (M15 S2): its own MIR (own fns + in-consumer monomorphs +
/// external declares for imported `pub` callees), its interface summary, and its transitive
/// dependency interface-hash set (the S3 cache-key input). Produced bottom-up by [`build_per_unit`].
pub struct PerUnitArtifact {
    pub unit: String,
    pub is_entry: bool,
    pub mir: MirProgram,
    pub summary: align_interface::InterfaceSummary,
    pub dep_interface_hashes: Vec<(String, align_interface::Hash128)>,
    /// The unit's source file path on disk — its basename is what `explain-opt`'s per-unit
    /// `DebugInfo` names, so LLVM's remarks attribute to the right file in the aggregated report.
    pub file: String,
}

/// The per-unit compilation result: one artifact per cleanly-checked unit (bottom-up), the FULL
/// per-order transitive dependency-hash list (an entry for every unit, whether or not it checked
/// cleanly — the S1b `check_per_unit` contract), and the union of all per-unit diagnostics. See
/// [`build_per_unit`].
pub struct PerUnitWalk {
    pub units: Vec<PerUnitArtifact>,
    pub dep_interface_hashes: Vec<(String, Vec<(String, align_interface::Hash128)>)>,
    pub diags: Diagnostics,
}

/// M15 S2 per-unit build (library entry): walk the import DAG bottom-up, check each unit against its
/// imports' interface summaries, and lower each cleanly-checked unit to its OWN MIR under the
/// separate-compilation visibility model. Returns one [`PerUnitArtifact`] per unit (MIR + summary +
/// dependency hashes), ready for per-unit codegen + N-object link. Additive: the whole-program
/// [`check`]/build path is untouched. On any error, the affected unit contributes no artifact and the
/// error is in `diags` (the caller must not link a partial build).
pub fn build_per_unit(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitWalk {
    walk_per_unit(source_map, name, src, false)
}

/// M15 S2b per-unit build with **source locations** — like [`build_per_unit`], but each unit's MIR is
/// lowered with `Block::stmt_lines` populated ([`lower_to_mir_per_unit_located`]). Used by
/// `alignc explain-opt`, which compiles each unit in isolation and captures LLVM's optimization
/// remarks per unit (the remarks need the debug locations to attribute back to user source).
pub fn build_per_unit_located(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitWalk {
    walk_per_unit(source_map, name, src, true)
}

/// M15 S1b: check every unit **per-unit** (see [`build_per_unit`] for the shared walk). This is the
/// check-only projection: it discards the per-unit MIR and returns just the summaries + dependency
/// hashes + diagnostics (the S1b dev-verb contract).
pub fn check_per_unit(source_map: &mut SourceMap, name: &str, src: &str) -> PerUnitCheck {
    let walk = walk_per_unit(source_map, name, src, false);
    let summaries = walk.units.into_iter().map(|u| u.summary).collect();
    PerUnitCheck { summaries, dep_interface_hashes: walk.dep_interface_hashes, diags: walk.diags }
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

/// M15 S2 per-unit lowering: lower ONE unit's checked HIR to MIR under the separate-compilation
/// visibility model — a non-entry `pub` function gets `external` linkage, and imported `pub` callees
/// become external declares (`align_mir::lower_program_per_unit`). The whole-program [`lower_to_mir`]
/// keeps every function `internal` and drops declares, so the default object stays byte-identical.
pub fn lower_to_mir_per_unit(hir: &align_sema::Program) -> align_mir::Program {
    align_mir::lower_program_per_unit(hir)
}

/// M15 S2b per-unit lowering **with source locations** — [`lower_to_mir_per_unit`] plus populated
/// `Block::stmt_lines` (`align_mir::lower_program_per_unit_located`). Used by `explain-opt`, which
/// compiles each unit in isolation and needs the debug locations for LLVM's per-unit remarks.
pub fn lower_to_mir_per_unit_located(hir: &align_sema::Program, source_map: &SourceMap) -> align_mir::Program {
    align_mir::lower_program_per_unit_located(hir, source_map)
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

/// Build the S3 codegen cache key (`docs/impl/10-cache-first-optimization.md` §6.2) for one unit. The
/// target-dependent components come from [`align_codegen_llvm::resolve_target_identity`] and the exact
/// LLVM version from [`align_codegen_llvm::llvm_version`] — the SAME resolution codegen uses, so a
/// cache hit implies byte-identical object bytes. `impl_hash` + `dep_interface_hashes` are the unit's
/// `PerUnitArtifact` fields; `exports` is sorted+deduped and `dep_interface_hashes` sorted by unit
/// name so semantically equivalent inputs share a key.
pub fn build_codegen_key(
    unit: &str,
    impl_hash: Hash128,
    dep_interface_hashes: &[(String, Hash128)],
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
) -> Result<CodegenKey, String> {
    let rt = align_codegen_llvm::resolve_target_identity(target).map_err(|e| e.to_string())?;
    let object_format = match target_object_format()? {
        ObjectFormat::Elf => 0u8,
        ObjectFormat::MachO => 1u8,
    };
    let mut dep_hashes = dep_interface_hashes.to_vec();
    dep_hashes.sort_by(|a, b| a.0.cmp(&b.0));
    let mut exp = exports.to_vec();
    exp.sort();
    exp.dedup();
    let rt_lto_digest = rt_lto.then(|| Hash128::of(rt_lto_bitcode()));
    Ok(CodegenKey {
        cache_format_version: cache::CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: cache::compiler_build_id(),
        frontend_schema: align_interface::FORMAT_VERSION,
        located: false,
        impl_hash,
        dep_interface_hashes: dep_hashes,
        exports: exp,
        target_triple: rt.triple,
        object_format,
        resolved_cpu: rt.cpu,
        resolved_features: rt.features,
        profile_name: profile.name().to_string(),
        pipeline: profile.pipeline().to_string(),
        codegen_opt: profile.codegen_opt_name().to_string(),
        reloc_model: rt.reloc_model.to_string(),
        code_model: rt.code_model.to_string(),
        llvm_version: align_codegen_llvm::llvm_version(),
        rt_lto,
        rt_lto_digest,
        cross_unit_opt_digest: Vec::new(),
        unit: unit.to_string(),
    })
}

/// Emit one unit's object **through the codegen cache** (`docs/impl/10-cache-first-optimization.md`).
/// On an enabled hit, the CAS blob is written verbatim to `obj` and no codegen runs; on a miss (or when
/// the cache is disabled) [`emit_object_file`] runs today's codegen verbatim into `obj` and — when
/// enabled — the object bytes are published to the CAS + index. Returns the structured
/// [`CacheOutcome`] (its `hit`/`miss_reason` the observability model the tests assert). When
/// `cache` is [`CacheContext::Disabled`] this is byte-for-byte the pre-S3a behavior.
#[allow(clippy::too_many_arguments)]
pub fn emit_object_cached(
    cache: &CacheContext,
    unit: &str,
    impl_hash: Hash128,
    dep_interface_hashes: &[(String, Hash128)],
    mir: &align_mir::Program,
    obj: &std::path::Path,
    target: BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: bool,
) -> Result<CacheOutcome, String> {
    // When the cache is disabled (the default), skip building the key entirely — the codegen-key
    // inputs (`compiler_build_id`'s one-time `alignc`-binary hash, `resolve_target_identity`,
    // `llvm_version`, `target_object_format`) are pure cache overhead a cache-off build must not pay.
    // This is the byte-identical, no-extra-I/O pre-S3a path (the same disabled miss `codegen` returns).
    if !cache.is_enabled() {
        emit_object_file(mir, obj, target, profile, exports, rt_lto)?;
        return Ok(cache::CacheOutcome {
            stage: cache::CacheStage::Codegen,
            unit: unit.to_string(),
            hit: false,
            miss_reason: None,
        });
    }
    let key = build_codegen_key(unit, impl_hash, dep_interface_hashes, &target, profile, exports, rt_lto)?;
    cache.codegen(&key, obj, |out| emit_object_file(mir, out, target.clone(), profile, exports, rt_lto))
}

/// M15 S3b: codegen every unit of a per-unit build into `obj_paths` (parallel over cache MISSES), the
/// `build`/`run`/`size` path. Two phases, per the settled S3 design:
///
/// 1. **Serial** cache lookups (they mutate no shared LLVM state and produce the ordering): for each
///    unit build its key and look it up; a HIT writes the object from the CAS immediately, a MISS is
///    queued for codegen. When the cache is disabled every unit is a miss and NO key work runs.
/// 2. **Parallel** codegen of the misses via `std::thread::scope` — `jobs` worker threads pull the
///    next miss through a shared atomic index; each runs [`emit_object_file`] (a fresh LLVM `Context`
///    per call) into its own `obj_paths[i]`, then publishes to the CAS. LLVM's native target is
///    initialized ONCE on this (main) thread before the scope, never racily inside a worker.
///
/// Determinism: results return in DAG (unit) index order regardless of which worker finished first;
/// the caller iterates the returned outcomes / the units' capability libs in that same order. `-j 1`
/// is byte-identical to any `-j N` (each object is produced by an independent single-threaded codegen;
/// only *which thread* runs it differs). A codegen error is reported for the lowest failing DAG index.
///
/// `obj_paths.len()` must equal `units.len()`. `build`/`run`/`size` pass no export roots (a unit's
/// `pub` fns are already external; the entry's `main` is the only linker root).
pub fn codegen_units_parallel(
    units: &[PerUnitArtifact],
    obj_paths: &[std::path::PathBuf],
    cache: &CacheContext,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
) -> Result<Vec<CacheOutcome>, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    assert_eq!(units.len(), obj_paths.len(), "one object path per unit");
    let n = units.len();
    // LLVM native-target init once, on the main thread, before any worker touches codegen.
    align_codegen_llvm::ensure_target_initialized().map_err(|e| e.to_string())?;

    let enabled = cache.is_enabled();
    // Phase 1 (serial): keys + lookups. `outcomes[i]` is set for every unit (hit or miss placeholder);
    // `misses` lists the DAG indices still needing codegen; `keys[i]` is retained for the miss publish.
    let mut keys: Vec<Option<CodegenKey>> = (0..n).map(|_| None).collect();
    let mut outcomes: Vec<Option<CacheOutcome>> = (0..n).map(|_| None).collect();
    let mut misses: Vec<usize> = Vec::new();
    for (i, unit) in units.iter().enumerate() {
        if enabled {
            let key = build_codegen_key(&unit.unit, unit.summary.impl_hash, &unit.dep_interface_hashes, target, profile, &[], rt_lto)?;
            match cache.lookup(&key, &obj_paths[i]) {
                cache::CacheLookup::Hit(o) => outcomes[i] = Some(o),
                cache::CacheLookup::Miss { reason } => {
                    outcomes[i] = Some(CacheOutcome { stage: CacheStage::Codegen, unit: unit.unit.clone(), hit: false, miss_reason: reason });
                    misses.push(i);
                }
            }
            keys[i] = Some(key);
        } else {
            outcomes[i] = Some(CacheOutcome { stage: CacheStage::Codegen, unit: unit.unit.clone(), hit: false, miss_reason: None });
            misses.push(i);
        }
    }

    // Phase 2 (parallel): produce the misses. Shared by reference into the scope; each worker only
    // reads `units`/`obj_paths`/`keys` and appends any error under a short-held lock.
    if !misses.is_empty() {
        use std::sync::atomic::AtomicBool;
        let worker_count = jobs.max(1).min(misses.len());
        let next = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        let errors = std::sync::Mutex::new(Vec::<(usize, String)>::new());
        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                scope.spawn(|| loop {
                    // Fail-fast: once any unit has errored, stop CLAIMING new work. Checked only
                    // between units — an in-progress `emit_object_file` is never interrupted. Codegen
                    // errors are rare (sema already validated in the walk), so this mainly bounds the
                    // wasted work when a *systemic* failure (e.g. disk full) would otherwise compile
                    // every remaining object before the build fails. `Relaxed` is correct: the flag is
                    // a best-effort early-exit hint that publishes no data (errors ride the Mutex, and
                    // the final read happens-after the scope join).
                    if failed.load(Ordering::Relaxed) {
                        break;
                    }
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= misses.len() {
                        break;
                    }
                    let i = misses[k];
                    let unit = &units[i];
                    if let Err(e) = emit_object_file(&unit.mir, &obj_paths[i], target.clone(), profile, &[], rt_lto) {
                        errors.lock().expect("codegen error lock").push((i, e));
                        failed.store(true, Ordering::Relaxed);
                        continue;
                    }
                    if let Some(key) = &keys[i] {
                        cache.publish_after_miss(key, &obj_paths[i]);
                    }
                });
            }
        });
        // Deterministic report: the lowest-DAG-index error among those collected. Fail-fast may leave
        // a higher-index unit unattempted, so in the rare case of MULTIPLE independent codegen failures
        // the set collected is timing-dependent; the *reported* error is still the lowest index present
        // (and such failures — e.g. disk full — carry the same cause across units anyway).
        let mut errs = errors.into_inner().expect("codegen error lock");
        if !errs.is_empty() {
            errs.sort_by_key(|(i, _)| *i);
            let (i, e) = &errs[0];
            return Err(format!("codegen failed for unit `{}`: {e}", units[*i].unit));
        }
    }

    Ok(outcomes.into_iter().map(|o| o.expect("every unit gets an outcome in phase 1")).collect())
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

/// The shared seed for the runtime-source content digest (M15 S3b). MUST equal
/// `RUNTIME_SRC_DIGEST_SEED` in `build.rs` — the digest baked there is compared here, so the two
/// algorithms + seed must agree (pinned by [`tests::runtime_src_digest_matches_baked`]).
const RUNTIME_SRC_DIGEST_SEED: u64 = 0x616C_6967_6E5F_7274; // "align_rt"

/// The runtime-source content digest baked at build time (`build.rs` → `cargo:rustc-env`), the
/// staleness reference. Empty only if the source tree was absent when `alignc` was built.
const BAKED_RUNTIME_SRC_DIGEST: &str = env!("ALIGN_RUNTIME_SRC_DIGEST");

/// A deterministic, **mtime-independent** content digest of every `*.rs` file under `dir` (recursive):
/// relative paths sorted, then each `(rel_path, len, bytes)` folded into one buffer and wyhashed.
/// `None` if the tree is absent/unreadable. Mirrors the identical routine in `build.rs` (same seed +
/// canonical form), so a digest computed here at link time is comparable to the one baked at build.
pub fn runtime_src_digest(dir: &std::path::Path) -> Option<String> {
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).ok()?;
        for entry in entries.flatten() {
            // `file_type()` (from the iterator, no extra `stat`) does not follow symlinks, so a
            // symlinked dir is not traversed (no cycles / no escaping the tree).
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.extension().is_some_and(|x| x == "rs") {
                let rel = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                files.push((rel, path));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut buf: Vec<u8> = Vec::new();
    for (rel, path) in &files {
        let bytes = std::fs::read(path).ok()?;
        buf.extend_from_slice(rel.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&bytes);
    }
    Some(format!("{:016x}", align_hash::wyhash(&buf, RUNTIME_SRC_DIGEST_SEED)))
}

/// The content digest of the runtime archive bytes (`docs/impl/10-cache-first-optimization.md` §6.3):
/// a future link-cache-key input, and the identity a shared/cross-host cache would key link actions on.
/// Not yet folded into any key (link caching is a later slice); exposed + tested now so the identity
/// exists. A `touch` of the archive leaves this unchanged (content-addressed); rebuilt bytes change it.
pub fn runtime_archive_digest() -> Result<Hash128, String> {
    let archive = runtime_archive()?;
    let bytes = std::fs::read(&archive).map_err(|e| format!("cannot read {}: {e}", archive.display()))?;
    Ok(Hash128::of(&bytes))
}

/// Fail loudly if `libalign_runtime.a` does not correspond to the current `align_runtime` source.
///
/// `align_driver` has no cargo dependency edge to the runtime *staticlib*, and a unit-test build
/// (`cargo test -p align_runtime`) recompiles only the test harness — neither refreshes the `.a`. So
/// editing the runtime and re-running the driver/tests without a full `cargo build` would silently
/// link a *stale* archive: wrong behavior and baffling test failures.
///
/// M15 S3b switched the check from **mtime** to a **content digest**: the current runtime-source digest
/// is compared to the one baked into this `alignc` at build time (`build.rs`). Since `alignc` and the
/// `.a` are produced by the same `cargo build`, a match means the `.a` is current — regardless of file
/// mtimes. This kills the false-stale papercut (a `git checkout`/`touch` bumps source mtimes without
/// changing content, which the old mtime check flagged as stale) while keeping the teeth: a real
/// source edit changes the digest and fails loud until `cargo build` refreshes both.
///
/// No-ops when the source tree is absent (an installed `alignc`), unreadable, or when no digest was
/// baked — it only ever turns a definitely-stale link into an error, never blocks a legitimate one.
fn ensure_archive_fresh(_archive: &std::path::Path) -> Result<(), String> {
    let src = std::path::Path::new(RUNTIME_SRC_DIR);
    if !src.is_dir() || BAKED_RUNTIME_SRC_DIGEST.is_empty() {
        return Ok(()); // installed binary / no baked reference: nothing to compare against
    }
    let Some(current) = runtime_src_digest(src) else {
        return Ok(()); // cannot read the source tree: do not block the build
    };
    if current != BAKED_RUNTIME_SRC_DIGEST {
        return Err(format!(
            "libalign_runtime.a is stale: the content of {} differs from what this `alignc` was \
             built against.\nThe driver has no cargo edge to the runtime staticlib, so run \
             `cargo build` to refresh the archive before linking.",
            src.display(),
        ));
    }
    Ok(())
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
    fn runtime_src_digest_is_content_based_and_mtime_independent() {
        let root = std::env::temp_dir().join(format!("align-driver-srcdigest-{}-{:p}", std::process::id(), &0u8 as *const _));
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).expect("create temp tree");

        // Empty (no `.rs`) → a digest of the empty buffer (Some, deterministic). A non-`.rs` is ignored.
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        let empty = runtime_src_digest(&root);
        assert!(empty.is_some(), ".txt is not counted; the empty-`.rs` tree still digests");

        // Add `.rs` at two levels → a specific content digest.
        std::fs::write(root.join("a.rs"), b"fn a() {}").unwrap();
        std::fs::write(sub.join("b.rs"), b"fn b() {}").unwrap();
        let d1 = runtime_src_digest(&root).expect("digest");
        assert_ne!(Some(&d1), empty.as_ref(), "adding source changes the digest");

        // A pure `touch` (rewrite identical bytes, new mtime) leaves the digest UNCHANGED — the
        // papercut fix: content, not mtime, drives staleness.
        std::fs::write(root.join("a.rs"), b"fn a() {}").unwrap();
        assert_eq!(runtime_src_digest(&root).as_deref(), Some(d1.as_str()), "identical content → identical digest (mtime-independent)");

        // A content CHANGE flips the digest (keeps the teeth).
        std::fs::write(root.join("a.rs"), b"fn a() { let _ = 1; }").unwrap();
        assert_ne!(runtime_src_digest(&root).as_deref(), Some(d1.as_str()), "changed content → changed digest");

        // A missing directory yields None (read_dir fails; not a panic).
        assert_eq!(runtime_src_digest(&root.join("does-not-exist")), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn runtime_src_digest_matches_baked() {
        // Pins that `build.rs`'s baked digest and lib's recompute agree (same algorithm + seed). In a
        // dev tree the source is unchanged since the last build, so the two MUST be equal. Skips only
        // if `alignc` was built without a source tree (installed binary): baked digest empty.
        if BAKED_RUNTIME_SRC_DIGEST.is_empty() {
            return;
        }
        let src = std::path::Path::new(RUNTIME_SRC_DIR);
        if let Some(current) = runtime_src_digest(src) {
            assert_eq!(current, BAKED_RUNTIME_SRC_DIGEST, "lib recompute must equal build.rs's baked digest (algorithm/seed drift)");
        }
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
