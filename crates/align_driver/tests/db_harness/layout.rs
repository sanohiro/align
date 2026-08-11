//! The `pkg.db` project layout: the eight package modules plus whatever a test adds.

use crate::common::{Proj, fixture};
use std::path::PathBuf;
use std::sync::LazyLock;

/// The libpq stub. Baked in deliberately: it is C, so it is not a `.align` source whose edits would
/// rebuild the crate, and `scripts/lint-ratchet.sh`'s `fixture-bake` rule only covers `.align`.
pub const PG_STUB_C: &str = include_str!("../fixtures/pkg_db_q2_postgres_stub.c");
/// The SQLite prepared-statement stub shared with `pkg_db_q4a`.
pub const SQLITE_Q4A_STUB_C: &str = include_str!("../fixtures/pkg_db_q4a_sqlite_stub.c");

/// The eight `pkg.db` package sources, read from disk ONCE.
///
/// A `LazyLock` rather than a per-call `fixture(...)`: `fixture` leaks its contents to `'static`,
/// so calling it per `Layout::new()` would leak a fresh copy of the 138 KB `db.align` on every
/// construction — hundreds of times across a suite.
static PACKAGE: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    vec![
        ("pkg/db.align", fixture("apps/db/pkg/db.align")),
        ("pkg/db/sqlite.align", fixture("apps/db/pkg/db/sqlite.align")),
        (
            "pkg/db/postgres.align",
            fixture("apps/db/pkg/db/postgres.align"),
        ),
        (
            "pkg/db/internal.align",
            fixture("apps/db/pkg/db/internal.align"),
        ),
        (
            "pkg/db/internal/resource.align",
            fixture("apps/db/pkg/db/internal/resource.align"),
        ),
        (
            "pkg/db/internal/descriptor.align",
            fixture("apps/db/pkg/db/internal/descriptor.align"),
        ),
        (
            "pkg/db/internal/sqlite.align",
            fixture("apps/db/pkg/db/internal/sqlite.align"),
        ),
        (
            "pkg/db/internal/postgres.align",
            fixture("apps/db/pkg/db/internal/postgres.align"),
        ),
    ]
});

/// Every counter name `PG_COUNTERS_ALIGN` prints, in dump order.
///
/// One registry, three consumers: the Align module below prints these names, the parser rejects
/// anything outside it, and `CounterExpect` validates that an expectation names a real counter.
/// `pg_counter_registry_matches_the_align_module` keeps this list and the module in step without
/// needing a C compiler or LLVM, so a counter added on one side and forgotten on the other fails in
/// milliseconds instead of surviving until someone happens to assert on it.
pub const PG_COUNTER_NAMES: &[&str] = &[
    "pg.connect_calls",
    "pg.finish_calls",
    "pg.encoding_calls",
    "pg.execute_calls",
    "pg.clear_calls",
    "pg.protocol_ok",
    "pg.protocol_error",
    "pg.nonblocking_calls",
    "pg.cancel_calls",
    "pg.consume_calls",
    "pg.last_timeout",
    "pg.prepare_calls",
    "pg.execute_prepared_calls",
    "pg.control_calls",
    "pg.deallocate_calls",
    "pg.delivered_rows",
];

/// The names `PG_COUNTERS_ALIGN` actually prints, recovered from its source.
///
/// A counter line is `print("pg.<name>")` immediately followed by `print(align_pg_...())`, so the
/// quoted `pg.` literals in dump order ARE the emitted names. Reading them back from the module
/// keeps the check honest: it compares the registry against the source of truth rather than against
/// a second hand-written list.
pub fn pg_counter_names_in_module() -> Vec<String> {
    PG_COUNTERS_ALIGN
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("print(\"pg.")?;
            let name = rest.strip_suffix("\")")?;
            Some(format!("pg.{name}"))
        })
        .collect()
}

/// The Align module that reads the libpq stub's counters and prints them in the wire format
/// [`crate::db_harness::counters`] parses.
///
/// The name is carried in-band next to each value, so a counter added, removed, or reordered on one
/// side is detected exactly rather than shifting every later value silently.
// NOTE: `r##"` (not `r#"`) — the body contains `print("#db-counters-begin")`, whose `"#`
// would otherwise terminate the raw string early.
const PG_COUNTERS_ALIGN: &str = r##"module pkg.db.testkit.pg

extern "C" {
  fn align_pg_reset()
  fn align_pg_connect_calls() -> i32
  fn align_pg_finish_calls() -> i32
  fn align_pg_encoding_calls() -> i32
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_consume_calls() -> i32
  fn align_pg_last_timeout() -> i32
  fn align_pg_prepare_calls() -> i32
  fn align_pg_execute_prepared_calls() -> i32
  fn align_pg_control_calls() -> i32
  fn align_pg_deallocate_calls() -> i32
  fn align_pg_q6_delivered_rows() -> i32
}

pub fn reset() {
  unsafe { align_pg_reset() }
}

pub fn dump() {
  unsafe {
    print("#db-counters-begin")
    print("pg.connect_calls")
    print(align_pg_connect_calls())
    print("pg.finish_calls")
    print(align_pg_finish_calls())
    print("pg.encoding_calls")
    print(align_pg_encoding_calls())
    print("pg.execute_calls")
    print(align_pg_execute_calls())
    print("pg.clear_calls")
    print(align_pg_clear_calls())
    print("pg.protocol_ok")
    print(align_pg_protocol_ok())
    print("pg.protocol_error")
    print(align_pg_protocol_error())
    print("pg.nonblocking_calls")
    print(align_pg_nonblocking_calls())
    print("pg.cancel_calls")
    print(align_pg_cancel_calls())
    print("pg.consume_calls")
    print(align_pg_consume_calls())
    print("pg.last_timeout")
    print(align_pg_last_timeout())
    print("pg.prepare_calls")
    print(align_pg_prepare_calls())
    print("pg.execute_prepared_calls")
    print(align_pg_execute_prepared_calls())
    print("pg.control_calls")
    print(align_pg_control_calls())
    print("pg.deallocate_calls")
    print(align_pg_deallocate_calls())
    print("pg.delivered_rows")
    print(align_pg_q6_delivered_rows())
    print("#db-counters-end")
  }
}
"##;

/// A `pkg.db` multi-file project under construction.
#[derive(Clone, Default)]
pub struct Layout {
    files: Vec<(String, String)>,
    c_sources: Vec<&'static str>,
}

impl Layout {
    /// The eight `pkg.db` package modules and nothing else.
    pub fn new() -> Layout {
        Layout {
            files: PACKAGE
                .iter()
                .map(|(p, s)| ((*p).to_string(), (*s).to_string()))
                .collect(),
            c_sources: Vec::new(),
        }
    }

    /// Add a module, or REPLACE the one already at `path`.
    ///
    /// Replacement is what makes the derived-layout idiom (`retain` the base, `push` the variant)
    /// unnecessary; that idiom was copied six times in `pkg_db_q5b2` alone.
    pub fn module(mut self, path: &str, src: &str) -> Layout {
        match self.files.iter_mut().find(|(p, _)| p == path) {
            Some(slot) => slot.1 = src.to_string(),
            None => self.files.push((path.to_string(), src.to_string())),
        }
        self
    }

    /// Set `main.align`.
    pub fn main(self, src: &str) -> Layout {
        self.module("main.align", src)
    }

    /// Link the libpq stub, WITHOUT adding the counters module.
    ///
    /// Use this when a program checks the stub's counters inline in Align. Adding the counters
    /// module would put an extra module into the compiled program, so a migration that is meant to
    /// be a pure refactor must use this and not [`Layout::with_pg_counters`].
    pub fn linking_pg_stub(mut self) -> Layout {
        if !self.c_sources.contains(&PG_STUB_C) {
            self.c_sources.push(PG_STUB_C);
        }
        self
    }

    /// Link the shared SQLite prepared-statement stub, WITHOUT its counters module.
    pub fn linking_sqlite_stub(mut self) -> Layout {
        if !self.c_sources.contains(&SQLITE_Q4A_STUB_C) {
            self.c_sources.push(SQLITE_Q4A_STUB_C);
        }
        self
    }

    /// Link the libpq stub AND add the Align module that reads its counters.
    ///
    /// The two arrive together and only together: the Align module declares `extern "C"` symbols
    /// that only this C source defines, so a layout could otherwise be built whose counter module
    /// has no definitions and fails at link time (harness cell H9). There is deliberately no way to
    /// add the counters module on its own.
    pub fn with_pg_counters(self) -> Layout {
        self.linking_pg_stub()
            .module("pkg/db/testkit/pg.align", PG_COUNTERS_ALIGN)
    }


    /// Remove the module at `path`.
    ///
    /// Only a probe needs this: it exists so a test can prove that a layout missing a package
    /// module produces a compile diagnostic rather than silently checking a smaller program.
    pub fn without(mut self, path: &str) -> Layout {
        self.files.retain(|(p, _)| p != path);
        self
    }

    /// Whether any C fixture must be compiled and linked.
    pub fn has_c_fixture(&self) -> bool {
        !self.c_sources.is_empty()
    }

    /// The concatenated C fixture, in the order the stubs were added.
    pub fn c_fixture(&self) -> Option<String> {
        if self.c_sources.is_empty() {
            return None;
        }
        Some(self.c_sources.join("\n"))
    }

    /// The driver-ready `(path, source)` view.
    pub fn files(&self) -> Vec<(&str, &str)> {
        self.files
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect()
    }

    /// The modules this TEST owns — everything except the eight `pkg.db` package sources.
    ///
    /// A case fingerprint uses this rather than [`Layout::files`]. The package sources are 138 KB
    /// of product code with their own owners, and folding them into a committed golden would make
    /// it churn on every `apps/db` edit while proving nothing about the test. What the fingerprint
    /// must pin is the part the test controls.
    pub fn test_owned_files(&self) -> Vec<(&str, &str)> {
        let package: Vec<&str> = PACKAGE.iter().map(|(p, _)| *p).collect();
        self.files
            .iter()
            .filter(|(p, _)| !package.contains(&p.as_str()))
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect()
    }

    /// The module paths, in layout order (for assertions about composition).
    pub fn paths(&self) -> Vec<&str> {
        self.files.iter().map(|(p, _)| p.as_str()).collect()
    }

    /// Write the layout into a fresh temp project.
    ///
    /// `common::Proj::write` does NOT create parent directories (the private `TempProject` does),
    /// so a nested layout such as `pkg/db/internal/resource.align` must mkdir first. Getting this
    /// wrong fails with a bare `NotFound` that names no path.
    pub fn materialize(&self, tag: &str) -> Proj {
        let proj = Proj::new(tag, &[], "main.align");
        for (path, src) in &self.files {
            let full = proj.dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)
                    .unwrap_or_else(|e| panic!("mkdir {}: {e}", parent.display()));
            }
            std::fs::write(&full, src).unwrap_or_else(|e| panic!("write {}: {e}", full.display()));
        }
        proj
    }
}
