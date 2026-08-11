//! The dual-driver parity owner.
//!
//! # Why one program instead of two
//!
//! `pkg_db_q4b`'s `complete_sqlite_parameter_and_row_matrix_is_exact` and
//! `complete_postgres_parameter_and_row_matrix_is_exact` are 134 and 136 lines that differ in 52 —
//! 81% identical — and part of that 52 is pure formatting drift of the same 16-field struct
//! literal. Keeping two copies identical by hand is the recorded `operation-matrix-completeness`
//! failure class (`FINDINGS.md`, 4 events / 3 PRs), which is past the threshold where CLAUDE.md
//! requires machinery.
//!
//! Both drivers return `Result<pkg.db.conn, pkg.db.Error>` and both query constructors produce the
//! same `pkg.db.query<P, R>`, so the driver can be a *runtime branch inside one program*. The body
//! under test is then structurally identical across drivers rather than identical by convention.
//!
//! # The sixth substitution point
//!
//! The two members of the pair differ in more than the driver: SQLite ran through
//! `build_and_run_multi_with_static_descriptors` (whole-program `check` +
//! `lower_to_mir_with_static_descriptors`) and PostgreSQL through `build_and_run_multi_with_c`
//! (`build_per_unit` + a linked C fixture). That is **two compiler pipelines**, not one pipeline
//! with two drivers.
//!
//! A parity program must use ONE pipeline for both members or it is not a parity test, and only the
//! per-unit + C-fixture pipeline can link the libpq stub. This engine therefore uses that pipeline
//! for both drivers — which means the whole-program static-descriptor path over the converted
//! matrix is no longer covered *here*, and a separate retained owner must keep it. See
//! `sqlite_full_matrix_retains_the_whole_program_static_descriptor_path` in `pkg_db_q4b.rs`; the
//! `owner-test-topology` finding ("Retain the whole-program execution owner") is exactly this
//! mistake.
//!
//! # Selection protocol
//!
//! `align_sema` rejects an `-> i32` argv `main` (argv requires `Result<(), Error>`), so passing the
//! case through argv would force every matrix program off the exit-code protocol. Selection is by
//! environment variable instead, and the program keeps `fn main() -> i32`.
//!
//! Align `match` patterns are variant names or `_` only — there is no string-literal pattern — so
//! case dispatch is an `if`/`else` chain whose final `else` returns [`UNKNOWN_CASE`]. That arm is
//! the drift detector: a table row whose name no longer exists in the program hits it.

use super::fingerprint::CaseFingerprint;
use super::layout::Layout;
use super::run::{Mismatch, Run, assert_no_mismatches};
use super::runner::RUNNER_PER_UNIT_C;
use crate::common::{
    BuildTarget, Profile, Proj, SourceMap, build_per_unit, emit_object_file, link_objects,
};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Returned by a program's dispatch when no case name matched.
pub const UNKNOWN_CASE: i32 = 99;
/// Returned when `ALIGN_DB_DRIVER` is absent.
pub const MISSING_DRIVER: i32 = 90;
/// Returned when `ALIGN_DB_CASE` is absent.
pub const MISSING_CASE: i32 = 91;
/// The case name that makes a program print its own case list.
pub const LIST_CASE: &str = "__list__";
/// Exit code of a successful `__list__` run.
pub const LIST_OK: i32 = 42;

/// Default per-case wall-clock cap. A hung case must not consume the suite.
pub const DEFAULT_CASE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    Sqlite,
    Postgres,
}

impl Driver {
    pub fn env_value(self) -> &'static str {
        match self {
            Driver::Sqlite => "sqlite",
            Driver::Postgres => "postgres",
        }
    }
    pub fn name(self) -> &'static str {
        self.env_value()
    }
}

/// What a case's class code must be on each driver.
#[derive(Clone, Debug)]
pub enum Expect {
    /// Both drivers must produce the same class code. The default and the point of the table.
    Same(i32),
    /// A deliberate, reviewed divergence. `why` must explain why the drivers cannot agree.
    Differs {
        sqlite: i32,
        postgres: i32,
        why: &'static str,
    },
    /// The case is meaningful on ONE driver only.
    ///
    /// This is for a row inside an otherwise shared matrix — not a licence to route a genuinely
    /// native-only surface through the parity table. A surface with no cross-driver counterpart at
    /// all (SQLite `PRAGMA` shape probing, libpq nonblocking/cancel state) keeps its conventional
    /// standalone owner.
    DriverOnly {
        driver: Driver,
        code: i32,
        why: &'static str,
    },
}

#[derive(Clone, Debug)]
pub struct ParityCase {
    pub name: &'static str,
    pub expect: Expect,
}

impl ParityCase {
    pub const fn same(name: &'static str, code: i32) -> ParityCase {
        ParityCase {
            name,
            expect: Expect::Same(code),
        }
    }
    pub const fn differs(
        name: &'static str,
        sqlite: i32,
        postgres: i32,
        why: &'static str,
    ) -> ParityCase {
        ParityCase {
            name,
            expect: Expect::Differs {
                sqlite,
                postgres,
                why,
            },
        }
    }
    pub const fn driver_only(
        name: &'static str,
        driver: Driver,
        code: i32,
        why: &'static str,
    ) -> ParityCase {
        ParityCase {
            name,
            expect: Expect::DriverOnly { driver, code, why },
        }
    }
}

/// A program compiled ONCE and spawned per (driver, case).
///
/// One compile of the eight-module `pkg.db` package (138 KB of `db.align` alone) dominates
/// everything else, so the matrix pays it once. One process per case also gives free stub-counter
/// isolation: no reset ordering to reason about.
pub struct ParityProgram {
    label: String,
    exe: PathBuf,
    dir: PathBuf,
    timeout: Duration,
    /// The suite-owned modules this program was actually compiled from, kept so a fingerprint
    /// reflects the built artifact rather than a layout rebuilt by the assertion.
    test_owned: Vec<(String, String)>,
    _proj: Proj,
}

impl ParityProgram {
    /// Compile `layout` through the per-unit + C-fixture pipeline.
    pub fn build(tag: &str, layout: &Layout) -> ParityProgram {
        assert!(
            layout.has_c_fixture(),
            "a parity program must link a C fixture (call Layout::linking_pg_stub or with_pg_counters); \
             without it the PostgreSQL member cannot run"
        );
        let proj = layout.materialize(tag);
        let entry = proj.dir.join("main.align");
        let entry_src = std::fs::read_to_string(&entry).expect("read entry");
        let mut sm = SourceMap::new();
        let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
        assert!(
            !walk.diags.has_errors(),
            "`{tag}` failed to build:\n{}",
            align_driver::format_diagnostics(&sm, &walk.diags)
        );
        let mut objects: Vec<PathBuf> = Vec::new();
        let mut link_libs: Vec<String> = Vec::new();
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
            .unwrap_or_else(|e| panic!("codegen for unit `{}`: {e}", unit.unit));
            for lib in &unit.mir.link_libs {
                // The C fixture supplies libpq's symbols; linking the real library too would let
                // the real one win and silently defeat the stub.
                if lib != "pq" && !link_libs.contains(lib) {
                    link_libs.push(lib.clone());
                }
            }
            objects.push(object);
        }
        let c_source = layout.c_fixture().expect("checked above");
        let c_path = proj.dir.join("fixture.c");
        let c_object = proj.dir.join("fixture.o");
        std::fs::write(&c_path, c_source).expect("write C fixture");
        let cc = Command::new("cc")
            .args(["-std=c11", "-c", "-O0"])
            .arg(&c_path)
            .arg("-o")
            .arg(&c_object)
            .output()
            .expect("launch cc");
        assert!(
            cc.status.success(),
            "compiling the C fixture failed: {}",
            String::from_utf8_lossy(&cc.stderr)
        );
        objects.push(c_object);
        let refs: Vec<&Path> = objects.iter().map(PathBuf::as_path).collect();
        let exe = proj
            .dir
            .join(format!("parity{}", std::env::consts::EXE_SUFFIX));
        link_objects(&refs, &exe, &link_libs, Profile::Release).expect("link parity program");
        ParityProgram {
            label: tag.to_string(),
            exe,
            dir: proj.dir.clone(),
            timeout: DEFAULT_CASE_TIMEOUT,
            test_owned: layout
                .test_owned_files()
                .into_iter()
                .map(|(p, s)| (p.to_string(), s.to_string()))
                .collect(),
            _proj: proj,
        }
    }

    /// The pipeline this program was compiled through.
    ///
    /// Exposed so a fingerprint golden reads the runner from the ENGINE instead of restating it. A
    /// golden that hardcodes the pipeline cannot notice the pipeline changing, which is the one
    /// substitution point most likely to change silently.
    pub fn runner_id(&self) -> &'static str {
        RUNNER_PER_UNIT_C
    }

    /// The exact environment [`ParityProgram::run_case`] sets on a child.
    ///
    /// The single source of truth: `run_case` builds the child from this, and the fingerprint
    /// golden hashes it, so a variable added to a child run shows up as a golden diff.
    pub fn case_env(&self, driver: Driver, case: &str) -> Vec<(&'static str, String)> {
        vec![
            ("ALIGN_DB_DRIVER", driver.env_value().to_string()),
            ("ALIGN_DB_CASE", case.to_string()),
        ]
    }

    /// The fingerprint of one `(case, driver)` AS THIS PROGRAM WOULD RUN IT.
    ///
    /// Every field is read back from the engine — the pipeline from [`ParityProgram::runner_id`],
    /// the child environment from [`ParityProgram::case_env`], the modules from the ones actually
    /// compiled. Nothing is restated by the caller except the expected class, which the parity
    /// table owns. A golden built this way notices a runner swap, a profile change, or a new child
    /// variable; a golden that rebuilt the inputs by hand would agree with itself and notice none
    /// of them.
    pub fn fingerprint(&self, case: &str, driver: Driver, expected_exit: i32) -> CaseFingerprint {
        let files: Vec<(&str, &str)> = self
            .test_owned
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect();
        let env = self.case_env(driver, case);
        let env_pairs: Vec<(&str, &str)> =
            env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        CaseFingerprint::new(
            format!("{}/{}/{}", self.label, case, driver.name()),
            self.runner_id(),
        )
        .files(&files)
        .env(&env_pairs)
        .expected_exit(expected_exit)
    }

    /// Spawn one child for `(driver, case)`.
    ///
    /// Both selectors are set EXPLICITLY and every other `ALIGN_DB_*` key inherited from the parent
    /// is removed. There is no implicit default: a program that reads a missing selector returns
    /// [`MISSING_DRIVER`]/[`MISSING_CASE`], which the engine treats as a failure rather than
    /// silently running "the default case".
    pub fn run_case(&self, driver: Driver, case: &str) -> Run {
        self.run_case_at(driver, case, case)
    }

    /// [`ParityProgram::run_case`] with an explicit capture-file slot, so two cases whose names
    /// sanitize to the same filename cannot overwrite each other's stdout.
    fn run_case_at(&self, driver: Driver, case: &str, slot: &str) -> Run {
        let label = format!("{}[{}/{}]", self.label, driver.name(), case);
        let stdout_path = self
            .dir
            .join(format!("out-{}-{}", driver.name(), sanitize(slot)));
        let stderr_path = self
            .dir
            .join(format!("err-{}-{}", driver.name(), sanitize(slot)));
        let mut command = Command::new(&self.exe);

        // Set first, then derive the removals from what was actually set: the clearing rule reads
        // the engine's own key list rather than a hardcoded copy of it.
        let explicit = self.case_env(driver, case);
        for (key, value) in &explicit {
            command.env(key, value);
        }
        let explicit_keys: Vec<&str> = explicit.iter().map(|(key, _)| *key).collect();
        // `vars_os` rather than `vars`: `vars` panics on a non-UTF-8 variable, and a developer's
        // shell is not obliged to keep every variable UTF-8. A key we cannot read as UTF-8 cannot
        // be an `ALIGN_DB_*` selector, so skipping it is correct.
        for (key, _) in std::env::vars_os() {
            let Some(key) = key.to_str() else { continue };
            if super::run::should_clear_env(key, &explicit_keys) {
                command.env_remove(key);
            }
        }

        command
            .stdout(File::create(&stdout_path).expect("create stdout capture"))
            .stderr(File::create(&stderr_path).expect("create stderr capture"));
        let mut child = command.spawn().expect("spawn parity case");
        let started = Instant::now();
        let (status, timed_out) = loop {
            match child.try_wait().expect("poll parity case") {
                Some(status) => break (status, false),
                None => {
                    if started.elapsed() >= self.timeout {
                        let _ = child.kill();
                        let status = child.wait().expect("reap timed-out parity case");
                        break (status, true);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let stdout = std::fs::read(&stdout_path).unwrap_or_default();
        let stderr = std::fs::read(&stderr_path).unwrap_or_default();
        let mut run = Run::new(label, std::process::Output { status, stdout, stderr });
        run.timed_out = timed_out;
        run
    }

    /// The case names the program reports for itself, under `driver`.
    pub fn list_cases(&self, driver: Driver) -> Vec<String> {
        let run = self.run_case_at(driver, LIST_CASE, LIST_CASE);
        if run.check_exit(LIST_OK).is_err() {
            panic!(
                "`{}` did not answer {LIST_CASE} with exit {LIST_OK}\n{}",
                self.label,
                run.describe()
            );
        }
        run.payload_lines()
    }
}

fn sanitize(case: &str) -> String {
    case.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Limits on a generated matrix, so a Cartesian product cannot silently become a 10,000-spawn
/// suite. [`run_parity`] applies [`Limits::default`]; a table that needs different ceilings calls
/// [`run_parity_with_limits`].
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_cases: usize,
    pub max_child_runs: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_cases: 512,
            max_child_runs: 1024,
        }
    }
}

/// Run the whole table on every driver and assert the declared parity.
///
/// `differs_budget` pins how many [`Expect::Differs`] rows the table is allowed to contain. It is a
/// ratchet: adding a divergence fails until the constant is raised in the same diff, which forces
/// the increase to be seen in review. Lowering it is always allowed.
/// Returns every run it made, so a caller can make further assertions (counter tables, payload)
/// without paying a second compile or a second spawn.
pub fn run_parity(
    program: &ParityProgram,
    cases: &[ParityCase],
    drivers: &[Driver],
    differs_budget: usize,
) -> Vec<(String, Driver, Run)> {
    run_parity_with_limits(program, cases, drivers, differs_budget, Limits::default())
}

pub fn run_parity_with_limits(
    program: &ParityProgram,
    cases: &[ParityCase],
    drivers: &[Driver],
    differs_budget: usize,
    limits: Limits,
) -> Vec<(String, Driver, Run)> {
    let mut mismatches: Vec<Mismatch> = Vec::new();
    let mut runs: Vec<(String, Driver, Run)> = Vec::new();

    // --- ceilings -----------------------------------------------------------------------------
    assert!(
        cases.len() <= limits.max_cases,
        "parity table `{}` has {} cases, over the declared ceiling of {}",
        program.label,
        cases.len(),
        limits.max_cases
    );
    let planned_runs = cases.len() * drivers.len() + drivers.len().min(1);
    assert!(
        planned_runs <= limits.max_child_runs,
        "parity table `{}` would spawn {planned_runs} children, over the declared ceiling of {}",
        program.label,
        limits.max_child_runs
    );

    // --- divergence budget --------------------------------------------------------------------
    let declared_differs = cases
        .iter()
        .filter(|c| matches!(c.expect, Expect::Differs { .. }))
        .count();
    assert!(
        declared_differs <= differs_budget,
        "parity table `{}` declares {declared_differs} divergent rows but its budget is \
         {differs_budget}; raise the budget in the same diff so the increase is reviewed",
        program.label
    );

    // --- duplicate row names --------------------------------------------------------------------
    let mut seen: Vec<&str> = Vec::new();
    for case in cases {
        assert!(
            !seen.contains(&case.name),
            "parity table `{}` lists case `{}` twice",
            program.label,
            case.name
        );
        seen.push(case.name);
    }

    // --- capture-path collisions ------------------------------------------------------------------
    // Case names become capture filenames through `sanitize`, which is not injective: `a.b` and
    // `a-b` both become `a_b`. Two such cases would overwrite each other's stdout and the second
    // would be checked against the first's counter dump.
    for (index, case) in cases.iter().enumerate() {
        for other in &cases[index + 1..] {
            assert!(
                sanitize(case.name) != sanitize(other.name),
                "parity table `{}`: cases `{}` and `{}` sanitize to the same capture filename `{}`",
                program.label,
                case.name,
                other.name,
                sanitize(case.name),
            );
        }
    }
    assert!(
        !cases.iter().any(|c| sanitize(c.name) == sanitize(LIST_CASE)),
        "parity table `{}` has a case whose capture filename collides with the `{LIST_CASE}` run",
        program.label,
    );

    // --- the table and the program must agree, on EVERY driver, in BOTH directions ----------------
    // Listing from only one driver would leave the other side unchecked, and a program whose
    // dispatch is itself driver-dependent could then hide a case from one of them.
    for &driver in drivers {
        let listed = program.list_cases(driver);
        for case in cases {
            if !listed.iter().any(|n| n == case.name) {
                mismatches.push(Mismatch {
                    what: format!("case `{}` in the Rust table", case.name),
                    expected: format!("listed by the program on {}", driver.name()),
                    actual: format!("{} lists {listed:?}", driver.name()),
                });
            }
        }
        for name in &listed {
            if !cases.iter().any(|c| c.name == name) {
                mismatches.push(Mismatch {
                    what: format!("case `{name}` listed by the program on {}", driver.name()),
                    expected: "present in the Rust parity table".to_string(),
                    actual: "absent from the table".to_string(),
                });
            }
        }
    }

    // --- run every (case, driver) and collect BEFORE asserting ----------------------------------
    // Collecting first means a mid-table failure or timeout never hides the rows after it.
    for case in cases {
        for &driver in drivers {
            let applicable = match case.expect {
                Expect::DriverOnly { driver: only, .. } => only == driver,
                _ => true,
            };
            if !applicable {
                continue;
            }
            let run = program.run_case(driver, case.name);
            let actual = run.output.status.code();
            let where_ = format!("case `{}` on {}", case.name, driver.name());

            if run.timed_out {
                mismatches.push(Mismatch {
                    what: where_,
                    expected: format!("completion within {:?}", program.timeout),
                    actual: format!("timed out and was killed; {}", run.describe()),
                });
            } else if actual == Some(UNKNOWN_CASE) {
                mismatches.push(Mismatch {
                    what: where_,
                    expected: "a name the program's dispatch knows".to_string(),
                    actual: format!(
                        "hit the unknown-case sentinel {UNKNOWN_CASE}: the table and the program \
                         disagree about this name"
                    ),
                });
            } else if actual == Some(MISSING_DRIVER) || actual == Some(MISSING_CASE) {
                mismatches.push(Mismatch {
                    what: where_,
                    expected: "both selectors visible to the child".to_string(),
                    actual: format!("the program reported a missing selector ({actual:?})"),
                });
            } else {
                let want = match (&case.expect, driver) {
                    (Expect::Same(code), _) => *code,
                    (Expect::Differs { sqlite, .. }, Driver::Sqlite) => *sqlite,
                    (Expect::Differs { postgres, .. }, Driver::Postgres) => *postgres,
                    (Expect::DriverOnly { code, .. }, _) => *code,
                };
                if actual != Some(want) {
                    let note = match &case.expect {
                        Expect::Differs { why, .. } => {
                            format!(" (declared divergence: {why})")
                        }
                        Expect::DriverOnly { why, .. } => {
                            format!(" ({} only: {why})", driver.name())
                        }
                        Expect::Same(_) => String::new(),
                    };
                    mismatches.push(Mismatch {
                        what: format!("{where_}{note}"),
                        expected: format!("Some({want})"),
                        actual: format!("{actual:?}\n{}", run.describe()),
                    });
                }
            }
            // Collected unconditionally and asserted only afterwards, so a failing or timed-out
            // case never hides the rows after it.
            runs.push((case.name.to_string(), driver, run));
        }
    }

    // --- P2-3: every row must actually have run -------------------------------------------------
    // A `DriverOnly` row whose driver is not in `drivers` matches nothing in the loop above and
    // would otherwise be reported as passing while never executing once. Silent non-execution is
    // the failure mode a parity table exists to prevent, so it is an error, not a skip.
    for case in cases {
        let executions = runs.iter().filter(|(name, _, _)| name == case.name).count();
        if executions == 0 {
            mismatches.push(Mismatch {
                what: format!("case `{}`", case.name),
                expected: "at least one execution".to_string(),
                actual: match case.expect {
                    Expect::DriverOnly { driver, .. } => format!(
                        "never ran: it is declared {} only, and {} is not among the drivers {:?}",
                        driver.name(),
                        driver.name(),
                        drivers.iter().map(|d| d.name()).collect::<Vec<_>>()
                    ),
                    _ => "never ran".to_string(),
                },
            });
        }
    }

    assert_no_mismatches(&format!("parity table `{}`", program.label), &mismatches);
    runs
}

/// The run for one `(case, driver)` from [`run_parity`]'s result.
pub fn run_of<'a>(
    runs: &'a [(String, Driver, Run)],
    case: &str,
    driver: Driver,
) -> &'a Run {
    runs.iter()
        .find(|(name, d, _)| name == case && *d == driver)
        .map(|(_, _, run)| run)
        .unwrap_or_else(|| panic!("no run recorded for case `{case}` on {}", driver.name()))
}
