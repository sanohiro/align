//! Native stub counters as data.
//!
//! A program used to end with a hand-numbered ladder:
//!
//! ```text
//! if align_pg_protocol_ok() != 1 { return 30 + align_pg_protocol_error() }
//! if align_pg_execute_calls() != 1 { return 27 }
//! if align_pg_clear_calls() != 1 { return 28 }
//! ```
//!
//! and `pkg_db_q4a` went further, collapsing eight counters into one `&&` chain returning a single
//! sentinel `6` — a failure that tells you one of eight numbers is wrong and nothing else. Of the
//! 92 counter checks in the corpus, **87 sit in exactly this trailing epilogue**; the other 5 are
//! mid-run deltas against a value captured earlier, which are a different assertion and stay in
//! Align.
//!
//! The 87 become a call to the shared `dump()` plus a table here, which reports *every* mismatching
//! counter by name.
//!
//! # Wire format
//!
//! ```text
//! #db-counters-begin
//! <name>
//! <value>
//! ...
//! #db-counters-end
//! ```
//!
//! The name travels next to its value, so a counter added, removed, or reordered on one side is
//! detected exactly instead of silently shifting every later value. Sentinel lines make a truncated
//! or absent dump detectable rather than parsing as "no counters", and they let a program's real
//! `print` output share stdout (`pkg_db_q2` already prints a native error message).

use super::run::{Mismatch, Run};
use std::collections::BTreeMap;

/// Counter values parsed from one run's stdout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    values: BTreeMap<String, i64>,
}

impl Counters {
    /// Parse every counter dump in `stdout`. Multiple dumps merge, and a name repeated with a
    /// different value is an error (two dumps of a changing counter would otherwise silently keep
    /// whichever came last).
    pub fn parse(stdout: &str) -> Result<Counters, String> {
        let mut values: BTreeMap<String, i64> = BTreeMap::new();
        let mut lines = stdout.lines();
        let mut saw_dump = false;
        while let Some(line) = lines.next() {
            if line != "#db-counters-begin" {
                continue;
            }
            saw_dump = true;
            loop {
                let name = lines
                    .next()
                    .ok_or_else(|| "counter dump ended without #db-counters-end".to_string())?;
                if name == "#db-counters-end" {
                    break;
                }
                if name == "#db-counters-begin" {
                    return Err("nested #db-counters-begin".to_string());
                }
                let raw = lines.next().ok_or_else(|| {
                    format!("counter `{name}` has no value before end of output")
                })?;
                let value: i64 = raw
                    .trim()
                    .parse()
                    .map_err(|_| format!("counter `{name}` has non-numeric value `{raw}`"))?;
                // An unknown name means the Align module and this registry have diverged. Accepting
                // it would let a renamed counter read as "absent" at the expectation instead of as
                // the schema break it is.
                if !super::stubs::is_known_counter(name) {
                    return Err(format!(
                        "counter `{name}` is not in the known registry {:?}",
                        super::stubs::all_counter_names()
                    ));
                }
                match values.get(name) {
                    Some(previous) if *previous != value => {
                        return Err(format!(
                            "counter `{name}` reported twice with different values ({previous} then {value})"
                        ));
                    }
                    _ => {
                        values.insert(name.to_string(), value);
                    }
                }
            }
        }
        if !saw_dump {
            return Err("no #db-counters-begin in stdout (did the program call dump()?)".to_string());
        }
        Ok(Counters { values })
    }

    pub fn get(&self, name: &str) -> Option<i64> {
        self.values.get(name).copied()
    }

    pub fn names(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }
}

/// A declarative expectation over a run's counters.
#[derive(Clone, Debug, Default)]
pub struct CounterExpect {
    want: Vec<(String, i64)>,
}

impl CounterExpect {
    pub fn new() -> CounterExpect {
        CounterExpect::default()
    }

    /// Expect `name == value`.
    ///
    /// The name is validated against the registry immediately: a typo would otherwise be reported
    /// as "counter absent from the dump", which reads like a product defect rather than a test bug.
    pub fn eq(mut self, name: &str, value: i64) -> CounterExpect {
        assert!(
            super::stubs::is_known_counter(name),
            "`{name}` is not a known counter; the registry has {:?}",
            super::stubs::all_counter_names()
        );
        self.want.push((name.to_string(), value));
        self
    }

    // ---- shorthands ---------------------------------------------------------------------------
    // Only the counters some suite actually asserts get a named method. Everything else in the
    // registry is reachable through `eq`, which validates the name, so a wrapper per counter would
    // be dormant surface with no added safety.
    /// The libpq stub's own protocol self-check; also pins `pg.protocol_error` to 0 so a failure
    /// names the reported error code instead of hiding it behind a sentinel offset.
    pub fn pg_protocol_ok(self) -> CounterExpect {
        self.eq("pg.protocol_ok", 1).eq("pg.protocol_error", 0)
    }
    pub fn pg_execute(self, n: i64) -> CounterExpect {
        self.eq("pg.execute_calls", n)
    }
    pub fn pg_clear(self, n: i64) -> CounterExpect {
        self.eq("pg.clear_calls", n)
    }
    pub fn pg_finish(self, n: i64) -> CounterExpect {
        self.eq("pg.finish_calls", n)
    }

    /// Every mismatch, never just the first.
    pub fn check(&self, run: &Run) -> Vec<Mismatch> {
        let parsed = match Counters::parse(&run.stdout()) {
            Ok(parsed) => parsed,
            Err(error) => {
                return vec![Mismatch {
                    what: format!("counter dump of `{}`", run.label),
                    expected: "a well-formed #db-counters block".to_string(),
                    actual: error,
                }];
            }
        };
        let mut mismatches = Vec::new();
        for (name, expected) in &self.want {
            match parsed.get(name) {
                Some(actual) if actual == *expected => {}
                Some(actual) => mismatches.push(Mismatch {
                    what: format!("counter `{name}` of `{}`", run.label),
                    expected: expected.to_string(),
                    actual: actual.to_string(),
                }),
                None => mismatches.push(Mismatch {
                    what: format!("counter `{name}` of `{}`", run.label),
                    expected: expected.to_string(),
                    actual: format!("absent (dump has {:?})", parsed.names()),
                }),
            }
        }
        mismatches
    }

    /// Check and panic with the complete mismatch list plus the run context.
    pub fn assert(&self, run: &Run) {
        let mismatches = self.check(run);
        if mismatches.is_empty() {
            return;
        }
        let body = mismatches
            .iter()
            .map(|m| format!("  - {m}"))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "{} counter expectation(s) failed\n{body}\n{}",
            mismatches.len(),
            run.describe()
        );
    }
}

/// Shorthand for a fresh libpq expectation that also pins the protocol self-check.
pub fn pg() -> CounterExpect {
    CounterExpect::new().pg_protocol_ok()
}
