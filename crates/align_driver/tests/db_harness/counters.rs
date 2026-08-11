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

    /// Expect `name == value`. Unknown names are caught at check time against the parsed dump.
    pub fn eq(mut self, name: &str, value: i64) -> CounterExpect {
        self.want.push((name.to_string(), value));
        self
    }

    // ---- libpq shorthands -----------------------------------------------------------------
    /// The stub's own protocol self-check; also pins `pg.protocol_error` to 0 so a failure names
    /// the reported error code instead of hiding it behind a sentinel offset.
    pub fn pg_protocol_ok(self) -> CounterExpect {
        self.eq("pg.protocol_ok", 1).eq("pg.protocol_error", 0)
    }
    pub fn pg_connect(self, n: i64) -> CounterExpect {
        self.eq("pg.connect_calls", n)
    }
    pub fn pg_finish(self, n: i64) -> CounterExpect {
        self.eq("pg.finish_calls", n)
    }
    pub fn pg_execute(self, n: i64) -> CounterExpect {
        self.eq("pg.execute_calls", n)
    }
    pub fn pg_clear(self, n: i64) -> CounterExpect {
        self.eq("pg.clear_calls", n)
    }
    pub fn pg_prepare(self, n: i64) -> CounterExpect {
        self.eq("pg.prepare_calls", n)
    }
    pub fn pg_execute_prepared(self, n: i64) -> CounterExpect {
        self.eq("pg.execute_prepared_calls", n)
    }
    pub fn pg_control(self, n: i64) -> CounterExpect {
        self.eq("pg.control_calls", n)
    }
    pub fn pg_deallocate(self, n: i64) -> CounterExpect {
        self.eq("pg.deallocate_calls", n)
    }
    pub fn pg_nonblocking(self, n: i64) -> CounterExpect {
        self.eq("pg.nonblocking_calls", n)
    }
    pub fn pg_cancel(self, n: i64) -> CounterExpect {
        self.eq("pg.cancel_calls", n)
    }
    pub fn pg_delivered_rows(self, n: i64) -> CounterExpect {
        self.eq("pg.delivered_rows", n)
    }

    // ---- SQLite shorthands ----------------------------------------------------------------
    pub fn sqlite_protocol_ok(self) -> CounterExpect {
        self.eq("sqlite.protocol_ok", 1)
    }
    pub fn sqlite_prepare(self, n: i64) -> CounterExpect {
        self.eq("sqlite.prepare_calls", n)
    }
    pub fn sqlite_finalize(self, n: i64) -> CounterExpect {
        self.eq("sqlite.finalize_calls", n)
    }
    pub fn sqlite_reset(self, n: i64) -> CounterExpect {
        self.eq("sqlite.reset_calls", n)
    }
    pub fn sqlite_clear(self, n: i64) -> CounterExpect {
        self.eq("sqlite.clear_calls", n)
    }
    pub fn sqlite_bind_i64(self, n: i64) -> CounterExpect {
        self.eq("sqlite.bind_i64_calls", n)
    }
    pub fn sqlite_bind_text(self, n: i64) -> CounterExpect {
        self.eq("sqlite.bind_text_calls", n)
    }
    pub fn sqlite_bind_blob(self, n: i64) -> CounterExpect {
        self.eq("sqlite.bind_blob_calls", n)
    }
    pub fn sqlite_busy_timeout(self, n: i64) -> CounterExpect {
        self.eq("sqlite.busy_timeout_calls", n)
    }
    pub fn sqlite_last_busy_timeout(self, n: i64) -> CounterExpect {
        self.eq("sqlite.last_busy_timeout", n)
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

/// Shorthand for a fresh SQLite expectation that also pins the protocol self-check.
pub fn sqlite() -> CounterExpect {
    CounterExpect::new().sqlite_protocol_ok()
}
