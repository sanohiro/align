//! Function-partition ThinLTO owner: formation, cache frontier, sealed completion, and support.

mod common;
use common::*;

const V1: &str = "\
fn leaf(n: i64) -> i64 = n + 1
fn sibling(n: i64) -> i64 = n * 2
fn main() -> i32 { print(leaf(41) + sibling(2)); return 0 }
";

const V2: &str = "\
fn leaf(n: i64) -> i64 = n + 2
fn sibling(n: i64) -> i64 = n * 2
fn main() -> i32 { print(leaf(41) + sibling(2)); return 0 }
";

const V3: &str = "\
fn leaf(n: i64) -> i64 = n + 1
fn sibling(n: i64) -> i64 = n * 3
fn main() -> i32 { print(leaf(41) + sibling(2)); return 0 }
";

fn walk(proj: &Proj) -> PerUnitWalk {
    let entry = proj.dir.join(&proj.entry);
    let source = std::fs::read_to_string(&entry).expect("read entry");
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &entry.display().to_string(), &source);
    assert!(
        !walk.diags.has_errors(),
        "unexpected build errors:\n{}",
        align_driver::format_diagnostics(&source_map, &walk.diags)
    );
    walk
}

fn function_prelinks(
    observations: &[FunctionThinLtoObservation],
) -> std::collections::BTreeMap<String, bool> {
    observations
        .iter()
        .filter_map(|observation| match observation {
            FunctionThinLtoObservation::Partitioned {
                source:
                    ThinPartitionSource {
                        partition: PartitionKey::Function(function),
                        ..
                    },
                prelink,
                ..
            } => Some((function.as_str().to_owned(), prelink.hit)),
            _ => None,
        })
        .collect()
}

fn function_backends(
    observations: &[FunctionThinLtoObservation],
) -> std::collections::BTreeMap<String, bool> {
    observations
        .iter()
        .filter_map(|observation| match observation {
            FunctionThinLtoObservation::Partitioned {
                source:
                    ThinPartitionSource {
                        partition: PartitionKey::Function(function),
                        ..
                    },
                backend,
                ..
            } => Some((function.as_str().to_owned(), backend.hit)),
            _ => None,
        })
        .collect()
}

#[test]
fn function_partition_cache_edit_revert_and_completion() {
    if !backend_available() {
        return;
    }
    let proj = Proj::new("function-cache", &[("main.align", V1)], "main.align");
    let cache = proj.cache();

    let cold_walk = walk(&proj);
    let cold = build_function_thin_lto(
        &cold_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("cold function ThinLTO build");
    assert_eq!(cold.mode(), FunctionThinLtoMode::Partitioned);
    assert_eq!(
        function_prelinks(cold.observations()),
        [
            ("leaf".to_owned(), false),
            ("main".to_owned(), false),
            ("sibling".to_owned(), false)
        ]
        .into_iter()
        .collect()
    );
    let executable = proj.dir.join("function-thin");
    cold.link_and_publish(&align_driver::CDriver::default(), &executable).expect("link cold build");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("run cold build");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"46\n");

    let hot_walk = walk(&proj);
    let hot = build_function_thin_lto(
        &hot_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        0,
    )
    .expect("hot function ThinLTO build");
    assert!(
        function_prelinks(hot.observations())
            .values()
            .all(|hit| *hit)
    );
    drop(hot);

    proj.write("main.align", V2);
    let edited_walk = walk(&proj);
    let edited = build_function_thin_lto(
        &edited_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("edited function ThinLTO build");
    assert_eq!(
        function_prelinks(edited.observations()),
        [
            ("leaf".to_owned(), false),
            ("main".to_owned(), true),
            ("sibling".to_owned(), true)
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        function_backends(edited.observations()),
        [
            ("leaf".to_owned(), false),
            ("main".to_owned(), false),
            ("sibling".to_owned(), true)
        ]
        .into_iter()
        .collect(),
        "the caller backend must bind the edited leaf partition digest"
    );
    drop(edited);

    proj.write("main.align", V1);
    let reverted_walk = walk(&proj);
    let reverted = build_function_thin_lto(
        &reverted_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        1,
    )
    .expect("reverted function ThinLTO build");
    assert!(
        function_prelinks(reverted.observations())
            .values()
            .all(|hit| *hit)
    );
    assert!(
        function_backends(reverted.observations())
            .values()
            .all(|hit| *hit)
    );
    drop(reverted);

    proj.write("main.align", V3);
    let sibling_walk = walk(&proj);
    let sibling = build_function_thin_lto(
        &sibling_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("sibling-edited function ThinLTO build");
    assert_eq!(
        function_prelinks(sibling.observations()),
        [
            ("leaf".to_owned(), true),
            ("main".to_owned(), true),
            ("sibling".to_owned(), false)
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        function_backends(sibling.observations()),
        [
            ("leaf".to_owned(), true),
            ("main".to_owned(), false),
            ("sibling".to_owned(), false)
        ]
        .into_iter()
        .collect(),
        "the caller backend must bind the independently edited sibling partition digest"
    );
}

#[test]
fn whole_unit_and_unknown_export_are_fail_closed() {
    let proj = Proj::new(
        "function-whole",
        &[("main.align", "fn main() -> i32 { return 0 }\n")],
        "main.align",
    );
    let walk = walk(&proj);
    let build = build_function_thin_lto(
        &walk.units,
        &CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        1,
    )
    .expect("whole-unit build");
    assert_eq!(build.mode(), FunctionThinLtoMode::WholeUnit);
    assert!(matches!(
        build.observations(),
        [FunctionThinLtoObservation::WholeUnit {
            source: ThinPartitionSource {
                partition: PartitionKey::WholeUnit,
                ..
            },
            codegen,
        }] if codegen.stage == CacheStage::Codegen
    ));

    let error = match function_partitions(&walk.units, &["missing".to_owned()]) {
        Ok(_) => panic!("unknown export unexpectedly formed partitions"),
        Err(error) => error,
    };
    assert_eq!(error, "unknown ThinLTO export roots:1:7:6d697373696e67");
}

#[test]
fn wrapped_and_direct_main_symbols_conflict_during_formation() -> Result<(), String> {
    let proj = Proj::new(
        "function-duplicate-main",
        &[
            ("dep.align", "module dep\npub fn helper() {}\n"),
            (
                "main.align",
                "module main\nimport dep\nfn main() -> i32 { return 0 }\n",
            ),
        ],
        "main.align",
    );
    let mut built = walk(&proj);
    let dependency = built
        .units
        .iter_mut()
        .find(|unit| !unit.is_entry)
        .ok_or_else(|| "dependency fixture unit missing".to_owned())?;
    let helper = dependency
        .mir
        .fns
        .first_mut()
        .ok_or_else(|| "dependency fixture function missing".to_owned())?;
    helper.name = align_mir::ProgramCall::try_from_logical("main")
        .map_err(|error| format!("cannot form main fixture identity: {error:?}"))?;
    helper.exportable = false;

    let error = match function_partitions(&built.units, &[]) {
        Ok(_) => return Err("duplicate C main symbols formed partitions".to_owned()),
        Err(error) => error,
    };
    assert_eq!(error, "duplicate ThinLTO root symbol:4:6d61696e");
    Ok(())
}

#[test]
fn size_verb_uses_function_partitions() {
    if !backend_available() {
        return;
    }
    let proj = Proj::new("function-size", &[("main.align", V1)], "main.align");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args([
            "size",
            "main.align",
            "--thin-lto",
            "--cache-stats",
            "--profile",
            "release",
        ])
        .current_dir(&proj.dir)
        .env("ALIGNC_CACHE", proj.cache_root())
        .output()
        .expect("run size --thin-lto");
    assert!(
        output.status.success(),
        "size --thin-lto failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("prelink: 0 hit, 3 miss") && stderr.contains("backend: 0 hit, 3 miss"),
        "size did not use the three function partitions:\n{stderr}"
    );
}

#[test]
fn resource_owner_forms_one_support_partition_and_runs_once() {
    if !backend_available() {
        return;
    }
    let internal = "module pkg.db.internal.resource\npub fn drop_conn(handle: raw) { unsafe { raw.free(handle) } }\n";
    let root = "module pkg.db\nimport pkg.db.internal.resource\npub resource conn = pkg.db.internal.resource.drop_conn\npub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }\n";
    let entry =
        "module main\nimport pkg.db\nfn main() -> i32 { owner := pkg.db.open(); return 0 }\n";
    let built = build_per_unit_multi(
        "function-resource",
        &[
            ("pkg/db/internal/resource.align", internal),
            ("pkg/db.align", root),
            ("main.align", entry),
        ],
        "main.align",
    );
    let partitions = function_partitions(&built.walk.units, &[]).expect("resource partitions");
    let supports = partitions
        .iter()
        .filter(|partition| matches!(partition.view, PartitionCodegenView::Support { .. }))
        .count();
    assert_eq!(
        supports,
        1,
        "exactly one producer support partition: {:?}",
        built
            .walk
            .units
            .iter()
            .map(|unit| (
                &unit.unit,
                unit.mir
                    .resources
                    .iter()
                    .map(|resource| (&resource.drop_hook, &resource.drop_thunk))
                    .collect::<Vec<_>>(),
                unit.mir
                    .fns
                    .iter()
                    .map(|function| function.name.as_str())
                    .collect::<Vec<_>>(),
            ))
            .collect::<Vec<_>>()
    );

    let executable = built.dir.join("resource-function-thin");
    let build = build_function_thin_lto(
        &built.walk.units,
        &CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("resource function ThinLTO build");
    build.link_and_publish(&align_driver::CDriver::default(), &executable).expect("resource link");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("resource run");
    assert!(
        output.status.success(),
        "resource executable failed: {output:?}"
    );
}

#[test]
fn shared_codegen_table_change_invalidates_every_function_partition() {
    let before = Proj::new(
        "function-shared-before",
        &[(
            "main.align",
            "Marker { value: i64 }\nfn leaf(n: i64) -> i64 = n + 1\nfn main() -> i32 { print(leaf(1)); return 0 }\n",
        )],
        "main.align",
    );
    let after = Proj::new(
        "function-shared-after",
        &[(
            "main.align",
            "Marker { value: u64 }\nfn leaf(n: i64) -> i64 = n + 1\nfn main() -> i32 { print(leaf(1)); return 0 }\n",
        )],
        "main.align",
    );
    let hashes = |proj: &Proj| {
        function_partitions(&walk(proj).units, &[])
            .expect("form shared-table partitions")
            .into_iter()
            .filter_map(|partition| match partition.view {
                PartitionCodegenView::Function { selected, .. } => {
                    Some((selected.name.as_str().to_owned(), partition.impl_hash))
                }
                PartitionCodegenView::Support { .. } => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before_hashes = hashes(&before);
    let after_hashes = hashes(&after);
    assert_eq!(
        before_hashes.keys().collect::<Vec<_>>(),
        after_hashes.keys().collect::<Vec<_>>()
    );
    assert!(
        before_hashes
            .iter()
            .all(|(function, hash)| after_hashes[function] != *hash),
        "one shared-table change must invalidate every function partition"
    );
}

/// Every owned-leaf arm of the call-provenance predicate, carried by the *second* edge of a
/// depth-2 same-unit chain. `main`'s partition declares only its direct callee, so the peer body
/// references a function the partition cannot see. Regression for #1070: the whole-program
/// validators were run unchanged over that truncated partition and rejected correct programs.
const DEPTH_TWO_CHAINS: &[(&str, &str, &[u8])] = &[
    (
        "str-arg-string-return",
        "fn leaf(text: str) -> string { return text.clone() }\n\
         fn middle(text: str) -> i64 = leaf(text).len()\n\
         fn main() -> i32 { print(middle(\"x\")); return 0 }\n",
        b"1\n",
    ),
    (
        "str-arg",
        "fn leaf(text: str) -> i64 { return text.len() }\n\
         fn middle(text: str) -> i64 = leaf(text) + 1\n\
         fn main() -> i32 { print(middle(\"x\")); return 0 }\n",
        b"2\n",
    ),
    (
        "string-return",
        "fn leaf() -> string { return \"abc\".clone() }\n\
         fn middle() -> i64 = leaf().len()\n\
         fn main() -> i32 { print(middle()); return 0 }\n",
        b"3\n",
    ),
    (
        "fn-value-arg",
        "fn bump(n: i64) -> i64 = n + 1\n\
         fn apply(f: fn(i64) -> i64, n: i64) -> i64 = f(n)\n\
         fn middle(n: i64) -> i64 = apply(bump, n)\n\
         fn main() -> i32 { print(middle(41)); return 0 }\n",
        b"42\n",
    ),
    (
        "capturing-closure",
        "fn middle(factor: i64) -> i64 = [1, 2, 3].map(fn x { x * factor }).sum()\n\
         fn main() -> i32 { print(middle(3)); return 0 }\n",
        b"18\n",
    ),
    (
        "i64-control",
        "fn leaf(n: i64) -> i64 = n + 1\n\
         fn middle(n: i64) -> i64 = leaf(n) * 2\n\
         fn main() -> i32 { print(middle(3)); return 0 }\n",
        b"8\n",
    ),
    (
        "peer-closed-chain",
        "fn leaf(text: str) -> string { return text.clone() }\n\
         fn middle(text: str) -> i64 = leaf(text).len()\n\
         fn main() -> i32 { print(middle(\"x\")); print(leaf(\"y\").len()); return 0 }\n",
        b"1\n1\n",
    ),
];

#[test]
fn depth_two_owned_leaf_chains_partition_and_run() {
    if !backend_available() {
        return;
    }
    for (label, source, expected) in DEPTH_TWO_CHAINS {
        let proj = Proj::new(&format!("depth-two-{label}"), &[("main.align", *source)], "main.align");
        let input = walk(&proj);
        let build = build_function_thin_lto(
            &input.units,
            &CacheContext::Disabled,
            &BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
            2,
        )
        .unwrap_or_else(|error| panic!("{label}: function ThinLTO build failed: {error}"));
        assert_eq!(
            build.mode(),
            FunctionThinLtoMode::Partitioned,
            "{label} must exercise function partitioning"
        );
        let executable = proj
            .dir
            .join(format!("depth-two{}", std::env::consts::EXE_SUFFIX));
        build
            .link_and_publish(&align_driver::CDriver::default(), &executable)
            .unwrap_or_else(|error| panic!("{label}: link failed: {error}"));
        let output = std::process::Command::new(&executable)
            .output()
            .unwrap_or_else(|error| panic!("{label}: run failed: {error}"));
        assert!(output.status.success(), "{label}: {output:?}");
        assert_eq!(output.stdout, *expected, "{label}");
    }
}

/// Validation scope must match emission scope, in both directions. A partition-scoped validator
/// must accept a reference it *cannot see* from a peer body, and must still reject the same defect
/// inside the body the partition emits.
#[test]
fn partition_scoped_validators_match_emission_scope() {
    use align_mir::producer::{
        validate_partition_resource_rvalues, validate_partition_tagged_program,
        validate_resource_rvalues, validate_tagged_program,
    };

    // The exact shape `emit_function_prelink_bc` builds: the selected root plus the peers its own
    // blocks reference, over the unit's complete shared tables. The deeper callee is absent.
    fn partition(source: &str, keep: &[&str]) -> align_mir::Program {
        let proj = Proj::new("partition-scope", &[("main.align", source)], "main.align");
        let unit = walk(&proj)
            .units
            .into_iter()
            .find(|unit| unit.is_entry)
            .expect("entry unit");
        let mut program = unit.mir;
        program
            .fns
            .retain(|function| keep.contains(&function.name.as_str()));
        assert_eq!(program.fns.len(), keep.len(), "partition fixture lost a function");
        program
    }

    let emitted = |name: &str| {
        std::collections::BTreeSet::from([
            align_mir::ProgramCall::try_from_logical(name).expect("fixture identity"),
        ])
    };

    // (a) Owned-leaf call provenance: `middle` calls `leaf`, which the partition cannot see.
    let owned_leaf = partition(DEPTH_TWO_CHAINS[0].1, &["main", "middle"]);
    assert!(
        validate_resource_rvalues(&owned_leaf).is_err(),
        "the whole-program validator must keep rejecting a truncated program"
    );
    assert!(
        validate_partition_resource_rvalues(&owned_leaf, &emitted("main")).is_ok(),
        "a peer body's unseen callee must not fail the partition"
    );

    // (b) The same relaxation must not reach the emitted body: retarget `main`'s own `str`-carrying
    // call at a function no table declares.
    let mut dangling = owned_leaf.clone();
    let absent = align_mir::ProgramCall::try_from_logical("absent").expect("absent identity");
    let mut retargeted = false;
    let root = dangling
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "main")
        .expect("emitted root");
    for block in &mut root.blocks {
        for statement in &mut block.stmts {
            match statement {
                align_mir::Stmt::Let(_, align_mir::Rvalue::Call(target, _)) => {
                    if let align_mir::DirectCall::Program(name) = target {
                        *name = absent.clone();
                        retargeted = true;
                    }
                }
                align_mir::Stmt::Let(_, align_mir::Rvalue::CallWithCleanup(call)) => {
                    call.target = absent.clone();
                    retargeted = true;
                }
                _ => {}
            }
        }
    }
    assert!(retargeted, "the emitted body must contain a direct program call");
    assert!(
        validate_partition_resource_rvalues(&dangling, &emitted("main")).is_err(),
        "an unresolvable call inside the emitted body must still be rejected"
    );

    // (c) Callable targets: `middle` takes the address of `bump`, which the partition cannot see.
    let fn_value = partition(DEPTH_TWO_CHAINS[3].1, &["main", "middle"]);
    assert!(
        validate_tagged_program(&fn_value).is_err(),
        "the whole-program validator must keep rejecting a truncated callable table"
    );
    assert!(
        validate_partition_tagged_program(&fn_value, &emitted("main")).is_ok(),
        "a peer body's unseen callable target must not fail the partition"
    );

    // …and only the *unseen* reference is exempt. A peer body whose callable target the partition
    // can resolve keeps every signature check.
    let mut resolvable = partition(DEPTH_TWO_CHAINS[3].1, &["main", "middle", "bump"]);
    let peer = resolvable
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "middle")
        .expect("peer body");
    let mut corrupted = false;
    for block in &mut peer.blocks {
        for statement in &mut block.stmts {
            if let align_mir::Stmt::Let(_, align_mir::Rvalue::FnAddr { signature, .. }) = statement {
                signature.param_modes.clear();
                corrupted = true;
            }
        }
    }
    assert!(corrupted, "the peer body must take a function address");
    assert!(
        validate_partition_tagged_program(&resolvable, &emitted("main")).is_err(),
        "a resolvable callable target keeps its signature check in every body"
    );

    // (d) …and the emitted body keeps its callable check: `main` itself takes the address here.
    let emitted_fn_value = partition(
        "fn bump(n: i64) -> i64 = n + 1\n\
         fn apply(f: fn(i64) -> i64, n: i64) -> i64 = f(n)\n\
         fn main() -> i32 { print(apply(bump, 41)); return 0 }\n",
        &["main", "apply"],
    );
    assert!(
        validate_partition_tagged_program(&emitted_fn_value, &emitted("main")).is_err(),
        "an unresolvable callable target inside the emitted body must still be rejected"
    );

    // (e) A partition validator is only as strong as the emitted set it is given, so a degenerate
    // one must fail closed rather than vacuously validate nothing.
    let empty = std::collections::BTreeSet::new();
    let absent_root = emitted("absent");
    for (label, scope) in [("empty", &empty), ("unknown", &absent_root)] {
        assert!(
            validate_partition_resource_rvalues(&owned_leaf, scope).is_err(),
            "{label} emitted set must be rejected by the resource-rvalue validator"
        );
        assert!(
            validate_partition_tagged_program(&owned_leaf, scope).is_err(),
            "{label} emitted set must be rejected by the tagged validator"
        );
    }
}

/// The partition-scoped validators are sound only because every unit is certified *whole* before
/// any partition is formed: a peer body the partition does not emit is trusted on that basis alone.
/// Nothing else in the suite pins that gate, so a later "skip revalidation on a warm unit" change
/// would silently leave peer bodies unvalidated. This test fails the moment formation stops
/// certifying a complete unit.
#[test]
fn unit_validation_precedes_partition_formation() {
    // The `fn`-value chain: `middle` takes the address of `bump`, and `middle` is a *peer* of
    // `main`'s partition, so its body statements are exactly what partition scope no longer
    // re-derives.
    let proj = Proj::new(
        "partition-unit-gate",
        &[("main.align", DEPTH_TWO_CHAINS[3].1)],
        "main.align",
    );
    let mut built = walk(&proj);
    let unit = built
        .units
        .iter_mut()
        .find(|unit| unit.is_entry)
        .expect("entry unit");
    let peer = unit
        .mir
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "middle")
        .expect("peer body");
    let mut corrupted = false;
    for block in &mut peer.blocks {
        for statement in &mut block.stmts {
            if let align_mir::Stmt::Let(_, align_mir::Rvalue::FnAddr { signature, .. }) = statement {
                signature.param_modes.clear();
                corrupted = true;
            }
        }
    }
    assert!(corrupted, "the peer body must take a function address");

    let error = match function_partitions(&built.units, &[]) {
        Ok(_) => panic!("a malformed unit body formed partitions"),
        Err(error) => error,
    };
    assert!(
        error.contains("ThinLTO program validation failed"),
        "formation must certify the complete unit before forming partitions, got: {error}"
    );
}

/// The source entry's return ABI must not decide whether partitioned builds
/// contain the C entry wrapper. The second function is the regression trigger.
#[test]
fn partitioned_entry_abis_match_the_whole_unit_shortcut() {
    if !backend_available() {
        return;
    }
    let cases = [
        ("unit", "fn main() { print(helper()) }", 0),
        ("i32", "fn main() -> i32 = helper() as i32", 42),
        (
            "ok",
            "fn main() -> Result<(), Error> { print(helper()); return Ok(()) }",
            0,
        ),
        (
            "err",
            "fn main() -> Result<(), Error> { return Err(error(helper() as i32)) }",
            42,
        ),
        (
            "argv",
            "fn main(args: array<str>) -> Result<(), Error> { if args.len() != 2 { return Err(error(11)) }; if args[1] != \"marker\" { return Err(error(12)) }; return Ok(()) }",
            0,
        ),
    ];
    for (name, entry, expected) in cases {
        for shape in 0..3 {
            let source = match shape {
                0 => entry.replace("helper()", "42"),
                1 => format!("{entry}\nfn helper() -> i64 = 42\n"),
                _ => format!(
                    "import dep\n{}\n",
                    entry.replace("helper()", "dep.helper()")
                ),
            };
            let mut files = vec![("main.align", source.as_str())];
            if shape == 2 {
                files.push(("dep.align", "module dep\npub fn helper() -> i64 = 42\n"));
            }
            let project = Proj::new(&format!("entry-{name}-{shape}"), &files, "main.align");
            let input = walk(&project);
            let cache = project.cache();
            for warm in [false, true] {
                let built = build_function_thin_lto(
                    &input.units,
                    &cache,
                    &BuildTarget::Baseline,
                    Profile::Release,
                    &[],
                    false,
                    2,
                )
                .expect("entry ThinLTO build");
                assert_eq!(
                    built.mode(),
                    if shape == 0 {
                        FunctionThinLtoMode::WholeUnit
                    } else {
                        FunctionThinLtoMode::Partitioned
                    }
                );
                if warm && shape != 0 {
                    assert!(
                        function_prelinks(built.observations())
                            .values()
                            .all(|hit| *hit)
                    );
                    assert!(
                        function_backends(built.observations())
                            .values()
                            .all(|hit| *hit)
                    );
                }
                let executable = project
                    .dir
                    .join(format!("entry{}", std::env::consts::EXE_SUFFIX));
                built
                    .link_and_publish(&align_driver::CDriver::default(), &executable)
                    .expect("entry must link");
                assert_eq!(
                    entry_exit(&executable),
                    expected,
                    "{name}, shape {shape}, warm {warm}"
                );
            }
        }
    }
}

fn entry_exit(executable: &std::path::Path) -> i32 {
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    // These generated fixtures call no process-spawning operation. Direct child
    // kill/reap therefore closes their complete process lifetime on every exit.
    struct Guard(Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Guard(
        Command::new(executable)
            .arg("marker")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn entry"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("poll entry") {
            return status.code().expect("entry was terminated by a signal");
        }
        assert!(
            Instant::now() < deadline,
            "entry exceeded its execution deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
