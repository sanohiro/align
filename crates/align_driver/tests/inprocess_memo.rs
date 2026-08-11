//! Owner tests for the process-in-memory compilation memo
//! (`docs/impl/10-cache-first-optimization.md` §6.6, closure matrix M1–M10).
//!
//! The memo is a pure optimization: reusing an artifact this process already produced from
//! byte-identical inputs must be indistinguishable from producing it again. Every test here is
//! therefore a two-part comparison — a memo-OFF baseline and a memo-ON replay — plus the hit/miss
//! accounting that proves the replay actually took the cached path rather than silently recomputing.
//!
//! The memo's counters are process-global, so the tests that read them serialize on [`memo_guard`].

mod common;
use common::*;

use align_driver::memo;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Serializes the tests that read the process-global memo counters, and restores the memo to its
/// default (enabled, empty) state on scope exit — including on a panic, so one failing assertion
/// cannot leave the memo disabled for every test that runs after it.
struct MemoGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for MemoGuard {
    fn drop(&mut self) {
        memo::set_enabled(true);
        memo::clear();
    }
}

/// Take the memo lock, then clear the memo and set `enabled`.
fn memo_guard(enabled: bool) -> MemoGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    memo::set_enabled(enabled);
    memo::clear();
    MemoGuard(guard)
}

/// One per-unit build's observable output: the unit order, each unit's structural MIR fingerprint
/// and interface identity, each unit's emitted object bytes, and the walk's rendered diagnostics.
///
/// This is everything a consumer of `build_per_unit` + `emit_object_file` can see, so two builds
/// with equal [`Built`] values are indistinguishable to every downstream stage (link, run, cache
/// key, interface publication).
#[derive(PartialEq, Eq, Debug)]
struct Built {
    units: Vec<String>,
    mir_hashes: Vec<Hash128>,
    interface_hashes: Vec<Hash128>,
    impl_hashes: Vec<Hash128>,
    objects: Vec<Vec<u8>>,
    /// Severity + message + span OFFSETS of every diagnostic, in order. The rendered form is
    /// deliberately not compared: it embeds the project's temp directory, which differs between two
    /// builds of the same sources by construction and says nothing about the memo.
    diagnostics: Vec<String>,
}

/// Build `files` per-unit in a FRESH project directory and record everything observable.
///
/// A fresh directory per build is deliberate: it is what a second test compiling the same package
/// actually does, and it proves the memo keys on content rather than on paths. Objects are emitted
/// only when the backend is available; without it the object vector stays empty and the frontend
/// half of every assertion still holds.
fn build(tag: &str, files: &[(&'static str, String)]) -> Built {
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let proj = Proj::new(tag, &refs, "main.align");
    let entry = proj.dir.join("main.align");
    let entry_src = std::fs::read_to_string(&entry).expect("read entry");
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
    let mut built = Built {
        units: walk.units.iter().map(|u| u.unit.clone()).collect(),
        mir_hashes: walk
            .units
            .iter()
            .map(|u| align_interface::codegen_impl_hash(&u.mir))
            .collect(),
        interface_hashes: walk.units.iter().map(|u| u.summary.interface_hash).collect(),
        impl_hashes: walk.units.iter().map(|u| u.summary.impl_hash).collect(),
        objects: Vec::new(),
        diagnostics: walk
            .diags
            .iter()
            .map(|diagnostic| {
                let span = diagnostic
                    .span
                    .map(|span| format!("{}..{}", span.lo, span.hi))
                    .unwrap_or_default();
                format!("{:?} {span} {}", diagnostic.severity, diagnostic.message)
            })
            .collect(),
    };
    if backend_available() {
        for (index, unit) in walk.units.iter().enumerate() {
            let object = proj.dir.join(format!("unit{index}.o"));
            emit_object_file(
                &unit.mir,
                &object,
                BuildTarget::Baseline,
                Profile::Release,
                &[],
                false,
            )
            .unwrap_or_else(|error| panic!("codegen for `{}`: {error}", unit.unit));
            built
                .objects
                .push(std::fs::read(&object).expect("read object"));
        }
    }
    built
}

/// The 4-unit DAG (`main → {b → c, d}`) with the given `c` body.
fn dag(cver: &str) -> Vec<(&'static str, String)> {
    dag_files(cver)
}

/// M1 + M2 + M6 + M9. A second compilation of byte-identical sources in a DIFFERENT directory
/// replays every unit and every object, and the replay is indistinguishable from a memo-off build.
#[test]
fn identical_sources_replay_every_unit_and_object() {
    let _guard = memo_guard(false);

    // Baseline: the memo is off, so this is exactly today's uncached pipeline.
    let baseline = build("memo-identical-off", &dag(C_V1));
    assert_eq!(memo::stats(), memo::MemoStats::default(), "memo off must not account anything");

    memo::set_enabled(true);
    let cold = build("memo-identical-cold", &dag(C_V1));
    let cold_stats = memo::stats();
    assert_eq!(cold, baseline, "a cold memo build must equal the memo-off build");
    assert_eq!(cold_stats.unit_hits, 0, "nothing is retained before the first build");
    assert_eq!(cold_stats.unit_misses, 4, "the DAG has four units");

    let warm = build("memo-identical-warm", &dag(C_V1));
    let warm_stats = memo::stats();
    assert_eq!(warm, baseline, "a replayed build must be byte-identical to the memo-off build");
    assert_eq!(
        warm_stats.unit_hits - cold_stats.unit_hits,
        4,
        "every unit must replay: {warm_stats:?}"
    );
    assert_eq!(
        warm_stats.unit_misses, cold_stats.unit_misses,
        "a full replay must not recompute a unit: {warm_stats:?}"
    );
    if backend_available() {
        assert_eq!(
            warm_stats.object_hits - cold_stats.object_hits,
            4,
            "every object must replay: {warm_stats:?}"
        );
        assert_eq!(
            warm_stats.object_misses, cold_stats.object_misses,
            "a full replay must not rerun codegen: {warm_stats:?}"
        );
        assert!(warm_stats.object_bytes > 0, "retained objects must be accounted");
    }
    assert_eq!(warm_stats.refused, 0, "the budgets must not bind at this size");
}

/// One whole-program build's observable output: the checked program's own diagnostics, its MIR
/// fingerprint, the module path that owns the entry function, and the emitted object bytes.
#[derive(PartialEq, Eq, Debug)]
struct WholeBuilt {
    entry_unit: String,
    mir_hash: Hash128,
    object: Vec<u8>,
    diagnostics: Vec<String>,
}

/// Compile `files` through the WHOLE-PROGRAM front end in a fresh project directory.
///
/// `source_map_prefix` files are added to the walk's `SourceMap` BEFORE the entry, which breaks the
/// canonical `unit i owns FileId i` assignment the program memo requires. A non-zero prefix must
/// therefore still compile correctly — with the memo declining to serve it.
fn build_whole(tag: &str, files: &[(&'static str, String)], source_map_prefix: usize) -> WholeBuilt {
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let proj = Proj::new(tag, &refs, "main.align");
    let entry = proj.dir.join("main.align");
    let entry_src = std::fs::read_to_string(&entry).expect("read entry");
    let mut sm = SourceMap::new();
    for index in 0..source_map_prefix {
        sm.add_file(format!("<prefix{index}>"), String::new());
    }
    let checked = check(&mut sm, &entry.display().to_string(), &entry_src);
    assert!(
        !checked.diags.has_errors(),
        "fixture must check: {}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let mut object = Vec::new();
    if backend_available() {
        let path = proj.dir.join("whole.o");
        emit_object_file(
            &mir,
            &path,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .expect("codegen");
        object = std::fs::read(&path).expect("read object");
    }
    WholeBuilt {
        entry_unit: checked.entry_unit.clone(),
        mir_hash: align_interface::codegen_impl_hash(&mir),
        object,
        diagnostics: checked
            .diags
            .iter()
            .map(|diagnostic| format!("{:?} {}", diagnostic.severity, diagnostic.message))
            .collect(),
    }
}

/// M1 + M6 for the whole-program stages. A second whole-program compilation of byte-identical
/// sources replays sema and lowering and produces the same program, MIR, and object.
#[test]
fn whole_program_check_and_lowering_replay() {
    let _guard = memo_guard(false);
    let baseline = build_whole("memo-whole-off", &dag(C_V1), 0);

    memo::set_enabled(true);
    build_whole("memo-whole-cold", &dag(C_V1), 0);
    let cold = memo::stats();
    assert_eq!(cold.program_hits, 0, "nothing is retained before the first build");

    let warm = build_whole("memo-whole-warm", &dag(C_V1), 0);
    let warm_stats = memo::stats();
    assert_eq!(warm, baseline, "a replayed whole-program build must equal the memo-off build");
    assert_eq!(warm_stats.program_hits - cold.program_hits, 1, "sema must replay: {warm_stats:?}");
    assert_eq!(
        warm_stats.program_misses, cold.program_misses,
        "a replayed sema must not recheck: {warm_stats:?}"
    );
    assert_eq!(
        warm_stats.lowering_hits - cold.lowering_hits,
        1,
        "lowering must replay: {warm_stats:?}"
    );

    // An edit anywhere in the source set is a different program.
    let before = memo::stats();
    build_whole("memo-whole-edited", &dag(C_BODY), 0);
    let after = memo::stats();
    assert_eq!(
        after.program_hits, before.program_hits,
        "an edited source set must recheck: {after:?}"
    );
    assert_eq!(
        after.lowering_hits, before.lowering_hits,
        "an edited source set must re-lower: {after:?}"
    );
}

/// M1 fail-closed. The program memo's replayed HIR and diagnostics carry `FileId`s, so it is used
/// only under the canonical `unit i owns FileId i` assignment. A walk handed a `SourceMap` that
/// already holds files still compiles correctly — it simply does not consult the memo.
#[test]
fn non_canonical_source_map_declines_the_program_memo() {
    let _guard = memo_guard(true);
    let canonical = build_whole("memo-canonical", &dag(C_V1), 0);
    let before = memo::stats();

    let shifted = build_whole("memo-shifted", &dag(C_V1), 3);
    let after = memo::stats();

    assert_eq!(
        shifted, canonical,
        "a shifted file-id assignment must still produce the same program"
    );
    assert_eq!(
        after.program_hits, before.program_hits,
        "the program memo must decline a non-canonical assignment: {after:?}"
    );
    assert_eq!(
        after.program_misses, before.program_misses,
        "declining must not even look the key up: {after:?}"
    );
}

/// M3. A PRIVATE body edit changes only that unit's key. Its dependents are checked against an
/// unchanged interface, so they replay — and the edited unit is genuinely recompiled, not served
/// the stale MIR.
#[test]
fn private_body_edit_misses_only_its_own_unit() {
    let _guard = memo_guard(true);

    let first = build("memo-body-first", &dag(C_V1));
    let before = memo::stats();
    let edited = build("memo-body-edited", &dag(C_BODY));
    let after = memo::stats();

    assert_eq!(after.unit_misses - before.unit_misses, 1, "only `c` may recompute: {after:?}");
    assert_eq!(after.unit_hits - before.unit_hits, 3, "`b`, `d`, `main` must replay: {after:?}");

    let c = first.units.iter().position(|u| u == "c").expect("unit c");
    assert_ne!(
        first.mir_hashes[c], edited.mir_hashes[c],
        "the edited unit must not be served the previous body"
    );
    if backend_available() {
        assert_ne!(
            first.objects[c], edited.objects[c],
            "the edited unit's object must reflect the new body"
        );
    }
    // The private edit leaves the public surface alone, which is why the dependents replayed.
    assert_eq!(
        first.interface_hashes[c], edited.interface_hashes[c],
        "a private body edit must not move the interface hash"
    );
}

/// M3. A PUBLIC surface edit changes the rendered interface every reverse dependent is checked
/// against, so each of them misses; an unrelated sibling still replays.
#[test]
fn public_surface_edit_misses_its_reverse_dependents() {
    let _guard = memo_guard(true);

    build("memo-surface-first", &dag(C_V1));
    let before = memo::stats();
    build("memo-surface-edited", &dag(C_PUB));
    let after = memo::stats();

    assert_eq!(
        after.unit_misses - before.unit_misses,
        3,
        "`c` and its reverse dependents `b` and `main` must recompute: {after:?}"
    );
    assert_eq!(
        after.unit_hits - before.unit_hits,
        1,
        "the independent unit `d` must still replay: {after:?}"
    );
}

/// M4. A unit that WARNS is still retained, and its warnings are replayed verbatim — same
/// severity, message, span offsets, and position in the walk's diagnostic list — with the span
/// reattached to the replaying walk's own file. Dropping a warning would make the memo observable;
/// excluding every warning-bearing unit would exclude exactly the expensive ones (`pkg.db` warns
/// sixteen times).
#[test]
fn warning_bearing_unit_replays_its_warnings() {
    let _guard = memo_guard(true);

    // `d` imports `c` without using it — an `unused import` WARNING, not an error, so the unit
    // still checks cleanly and still produces an artifact.
    let mut files = dag(C_V1);
    for (name, source) in &mut files {
        if *name == "d.align" {
            *source = "module d\nimport c\npub fn dval() -> i64 = 5\n".to_string();
        }
    }

    let first = build("memo-warned-first", &files);
    let before = memo::stats();
    let second = build("memo-warned-second", &files);
    let after = memo::stats();

    assert!(
        first
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("unused import")),
        "fixture must warn: {:?}",
        first.diagnostics
    );
    assert_eq!(
        second, first,
        "a replayed build must reproduce the same diagnostics, in the same order, at the same spans"
    );
    assert_eq!(
        after.unit_misses - before.unit_misses,
        0,
        "a warning does not disqualify a unit: {after:?}"
    );
    assert_eq!(
        after.unit_hits - before.unit_hits,
        4,
        "every unit — including the warned one — must replay: {after:?}"
    );

    // The replayed span must resolve through the REPLAYING walk's source map, not the one that
    // produced it: the rendered warning names the second project's own file.
    let rendered = render_diagnostics("memo-warned-rendered", &files);
    assert!(
        rendered.contains("memo-warned-rendered")
            && rendered.contains("d.align")
            && rendered.contains("unused import"),
        "a replayed diagnostic must render against the replaying project: {rendered}"
    );
}

/// Build per-unit in a fresh project and return the FULLY RENDERED diagnostics (file name, line,
/// column), which is what a user sees and what a stale span would corrupt.
fn render_diagnostics(tag: &str, files: &[(&'static str, String)]) -> String {
    let refs: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let proj = Proj::new(tag, &refs, "main.align");
    let entry = proj.dir.join("main.align");
    let entry_src = std::fs::read_to_string(&entry).expect("read entry");
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
    align_driver::format_diagnostics(&sm, &walk.diags)
}

/// M5. The process-global compiler toggles are key material.
///
/// `ALIGN_CONST_POOL` (sema), `ALIGN_NEEDLE_HOIST`, `ALIGN_BUFFER_DONATE`, and `ALIGN_SORT_ADAPTIVE`
/// (MIR lowering) are read at each lowering, and the measurement suites flip them with `set_var`
/// inside one test binary. A flipped toggle must therefore address a DIFFERENT key for every unit,
/// whether or not this particular fixture's output happens to change under it — the memo cannot know
/// which sources a toggle reaches. Flipping back must re-address the original artifacts.
#[test]
fn compiler_env_toggle_is_key_material() {
    let _guard = memo_guard(true);

    build("memo-toggle-default", &dag(C_V1));
    let before = memo::stats();

    // SAFETY: `memo_guard` holds the file-wide lock, so no test in this binary observes the write.
    unsafe { std::env::set_var("ALIGN_CONST_POOL", "off") };
    build("memo-toggle-off", &dag(C_V1));
    let toggled = memo::stats();
    unsafe { std::env::remove_var("ALIGN_CONST_POOL") };
    build("memo-toggle-restored", &dag(C_V1));
    let restored = memo::stats();

    assert_eq!(
        toggled.unit_hits - before.unit_hits,
        0,
        "a flipped toggle must not be served artifacts built under the opposite setting: {toggled:?}"
    );
    assert_eq!(
        toggled.unit_misses - before.unit_misses,
        4,
        "every unit must be rechecked under the flipped toggle: {toggled:?}"
    );
    assert_eq!(
        restored.unit_hits - toggled.unit_hits,
        4,
        "restoring the toggle must re-address the original artifacts: {restored:?}"
    );
}

/// M7. The memo is shared by the parallel per-unit codegen workers, so concurrent builds of the
/// same sources must agree with the serial result and with each other.
#[test]
fn concurrent_builds_agree_with_the_serial_result() {
    let _guard = memo_guard(false);
    let baseline = build("memo-parallel-baseline", &dag(C_V1));

    memo::set_enabled(true);
    memo::clear();
    let results: Vec<Built> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|worker| {
                std::thread::Builder::new()
                    .stack_size(32 * 1024 * 1024)
                    .spawn_scoped(scope, move || build(&format!("memo-parallel-{worker}"), &dag(C_V1)))
                    .expect("spawn build worker")
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("build worker"))
            .collect()
    });

    for (worker, result) in results.iter().enumerate() {
        assert_eq!(*result, baseline, "worker {worker} diverged from the serial build");
    }
    let stats = memo::stats();
    assert_eq!(
        stats.unit_hits + stats.unit_misses,
        16,
        "four workers must each look up four units: {stats:?}"
    );
}

/// M8. Turning the memo off is a complete bypass: no lookups, no retention, no accounting — and the
/// build it produces is the same one the memo would have replayed.
#[test]
fn disabled_memo_is_a_complete_bypass() {
    let _guard = memo_guard(true);
    let warm = build("memo-disabled-warm-a", &dag(C_V1));
    build("memo-disabled-warm-b", &dag(C_V1));
    let warm_stats = memo::stats();
    assert!(warm_stats.unit_hits > 0, "precondition: the memo was serving");

    memo::set_enabled(false);
    let off = build("memo-disabled-off", &dag(C_V1));
    assert_eq!(memo::stats(), warm_stats, "a disabled memo must not account anything");
    assert_eq!(off, warm, "a disabled build must equal the replayed build");
}
