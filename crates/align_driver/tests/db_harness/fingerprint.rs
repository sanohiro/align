//! Case fingerprints: the mechanical proof that a Layer 1 migration changed nothing.
//!
//! Layer 1 of the harness (layout builder, gate, exit assertion) is a pure refactor: the inputs
//! handed to the driver must be *identical* before and after. A fingerprint over those inputs, dumped
//! on the pre-migration commit and re-dumped after, proves it for the whole surface at once.
//!
//! # Why the fingerprint covers the whole invocation
//!
//! Hashing only `(path, source)` is not enough. A refactor can preserve the file set exactly while
//! silently changing:
//!
//! * the **runner** — for example from `build_and_run_multi_with_static_descriptors`
//!   (whole-program) to `build_and_run_multi_with_c` (per-unit + C fixture), which is a different
//!   compiler pipeline;
//! * the **environment** or **argv** handed to the child;
//! * the **expected exit code** the assertion checks.
//!
//! Every one of those changes what the test proves while leaving a file-only hash untouched, so all
//! of them are inside the fingerprint.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// FNV-1a. Deterministic and dependency-free; this detects change, it is not a security digest.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// Everything that determines what one case actually exercises.
pub struct CaseFingerprint {
    pub label: String,
    /// Which compile/execute pipeline runs the case: `"static_descriptors"`, `"per_unit_c"`,
    /// `"check_only"`, `"per_unit_check"`, …
    pub runner_id: &'static str,
    pub files: Vec<(String, String)>,
    pub env: BTreeMap<String, String>,
    pub argv: Vec<String>,
    pub expected_exit: Option<i32>,
}

impl CaseFingerprint {
    pub fn new(label: impl Into<String>, runner_id: &'static str) -> CaseFingerprint {
        CaseFingerprint {
            label: label.into(),
            runner_id,
            files: Vec::new(),
            env: BTreeMap::new(),
            argv: Vec::new(),
            expected_exit: None,
        }
    }

    pub fn files(mut self, files: &[(&str, &str)]) -> CaseFingerprint {
        self.files = files
            .iter()
            .map(|(p, s)| ((*p).to_string(), (*s).to_string()))
            .collect();
        self
    }

    pub fn env(mut self, pairs: &[(&str, &str)]) -> CaseFingerprint {
        // Sorted by construction: BTreeMap. Environment order must not perturb the digest.
        self.env = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        self
    }

    pub fn argv(mut self, argv: &[&str]) -> CaseFingerprint {
        self.argv = argv.iter().map(|a| (*a).to_string()).collect();
        self
    }

    pub fn expected_exit(mut self, code: i32) -> CaseFingerprint {
        self.expected_exit = Some(code);
        self
    }

    /// The digest, over every field, with `\0` separators so no two field boundaries can alias.
    pub fn digest(&self) -> u64 {
        let mut buffer: Vec<u8> = Vec::new();
        let push = |bytes: &[u8], buffer: &mut Vec<u8>| {
            buffer.extend_from_slice(bytes);
            buffer.push(0);
        };
        push(self.label.as_bytes(), &mut buffer);
        push(self.runner_id.as_bytes(), &mut buffer);
        for (path, source) in &self.files {
            push(path.as_bytes(), &mut buffer);
            push(source.as_bytes(), &mut buffer);
        }
        push(b"--env--", &mut buffer);
        for (key, value) in &self.env {
            push(key.as_bytes(), &mut buffer);
            push(value.as_bytes(), &mut buffer);
        }
        push(b"--argv--", &mut buffer);
        for argument in &self.argv {
            push(argument.as_bytes(), &mut buffer);
        }
        push(b"--exit--", &mut buffer);
        push(
            match self.expected_exit {
                Some(code) => code.to_string(),
                None => "none".to_string(),
            }
            .as_bytes(),
            &mut buffer,
        );
        fnv1a(&buffer)
    }
}

/// An ordered set of case fingerprints, rendered as a stable golden text.
#[derive(Default)]
pub struct FingerprintLog {
    rows: Vec<(String, u64)>,
}

impl FingerprintLog {
    pub fn new() -> FingerprintLog {
        FingerprintLog::default()
    }

    pub fn record(&mut self, fingerprint: &CaseFingerprint) {
        self.rows
            .push((fingerprint.label.clone(), fingerprint.digest()));
    }

    /// `<label> <digest>` per line, sorted by label so test execution order cannot perturb it.
    pub fn render(&self) -> String {
        let mut rows = self.rows.clone();
        rows.sort();
        let mut out = String::new();
        for (label, digest) in rows {
            let _ = writeln!(out, "{label} {digest:016x}");
        }
        out
    }

    /// Compare against a stored golden, reporting added, removed, and changed rows separately.
    pub fn assert_matches(&self, golden: &str) {
        let actual = self.render();
        if actual == golden {
            return;
        }
        let parse = |text: &str| -> BTreeMap<String, String> {
            text.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| l.split_once(' '))
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        let want = parse(golden);
        let got = parse(&actual);
        let mut report = String::new();
        for (label, digest) in &want {
            match got.get(label) {
                None => {
                    let _ = writeln!(report, "  removed: {label}");
                }
                Some(other) if other != digest => {
                    let _ = writeln!(report, "  changed: {label} {digest} -> {other}");
                }
                Some(_) => {}
            }
        }
        for label in got.keys() {
            if !want.contains_key(label) {
                let _ = writeln!(report, "  added:   {label}");
            }
        }
        panic!(
            "case fingerprints differ from the pre-migration golden\n{report}\n\
             A changed row means the migration altered what that case exercises (files, runner, \
             environment, argv, or expected exit), not just how it is written."
        );
    }
}
