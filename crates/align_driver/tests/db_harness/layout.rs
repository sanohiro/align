//! The `pkg.db` project layout: the package modules plus whatever a test adds.

use super::stubs::Stub;
use crate::common::{Proj, fixture};
use std::sync::LazyLock;

/// The eleven `pkg.db` package sources, read from disk ONCE.
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
        (
            "pkg/db/internal/postgres_status.align",
            fixture("apps/db/pkg/db/internal/postgres_status.align"),
        ),
        ("pkg/db/pool.align", fixture("apps/db/pkg/db/pool.align")),
        (
            "pkg/db/pool/internal/resource.align",
            fixture("apps/db/pkg/db/pool/internal/resource.align"),
        ),
    ]
});

/// One `pkg.db` package source, by layout path (`"pkg/db.align"`, `"pkg/db/sqlite.align"`, …).
///
/// Surface assertions read the shipped source directly. Going through the harness reuses the copy
/// `PACKAGE` already loaded instead of leaking another via `fixture`, and keeps every suite naming
/// the same package paths.
pub fn package_source(path: &str) -> &'static str {
    PACKAGE
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .map(|(_, source)| *source)
        .unwrap_or_else(|| {
            panic!(
                "`{path}` is not a pkg.db package module; the package has {:?}",
                PACKAGE.iter().map(|(p, _)| *p).collect::<Vec<_>>()
            )
        })
}

/// A `pkg.db` multi-file project under construction.
#[derive(Clone, Default)]
pub struct Layout {
    files: Vec<(String, String)>,
    c_sources: Vec<&'static Stub>,
}

impl Layout {
    /// The eleven `pkg.db` package modules and nothing else.
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

    /// Link `stub`'s C source WITHOUT adding its counters module.
    ///
    /// Use this when a program reads the stub's counters inline in Align. Adding the counters
    /// module would put an extra module into the compiled program, so a migration meant to be a
    /// pure refactor must use this and not [`Layout::with_counters`].
    pub fn linking(mut self, stub: &'static Stub) -> Layout {
        // Keyed on `id`, not address: `&PG` is a reference to a `const`, which the compiler may
        // promote to any number of distinct statics, so pointer identity between two `&PG`
        // expressions is not guaranteed. `id` is the stub's declared identity and is what the
        // fingerprint records too.
        if !self.c_sources.iter().any(|linked| linked.id == stub.id) {
            self.c_sources.push(stub);
        }
        self
    }

    /// Link `stub` AND add the Align module that reads its counters.
    ///
    /// The two arrive together and only together: the Align module declares `extern "C"` symbols
    /// that only this C source defines, so a layout could otherwise be built whose counters module
    /// has no definitions and fails at link time (harness cell H9). There is deliberately no way to
    /// add a counters module on its own.
    pub fn with_counters(self, stub: &'static Stub) -> Layout {
        let path = stub.counters_path;
        let source = stub.counters_align;
        self.linking(stub).module(path, source)
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
        Some(
            self.c_sources
                .iter()
                .map(|stub| stub.c_source)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// The driver-ready `(path, source)` view.
    pub fn files(&self) -> Vec<(&str, &str)> {
        self.files
            .iter()
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .collect()
    }

    /// The modules this TEST owns — everything except the eleven `pkg.db` package sources.
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
