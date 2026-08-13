//! The native stubs and their counter modules, as one table.
//!
//! Each stub is a C fixture plus an Align module that prints its counters plus the registry of the
//! names that module prints. Keeping the three together is the point: a counter added to the C but
//! not the Align module, or to the Align module but not the registry, is caught by one
//! parameterized owner (`counter_registries_match_their_align_modules`) that needs neither a C
//! compiler nor LLVM.
//!
//! Counter names are namespaced (`pg.`, `sqlite.`, `sqlite6.`) because a program may link several
//! stubs at once and dump them all into one stdout stream.

/// A native stub: its C source, its counters module, and the names that module prints.
pub struct Stub {
    /// Stable identity, used in case fingerprints.
    pub id: &'static str,
    pub c_source: &'static str,
    /// Where the counters module is written in the layout.
    pub counters_path: &'static str,
    pub counters_align: &'static str,
    /// Every counter name `counters_align` prints, in dump order.
    pub names: &'static [&'static str],
}

impl Stub {
    /// The names the Align module actually prints, recovered from its source.
    ///
    /// A counter line is `print("<ns>.<name>")` followed by `print(<getter>())`, so the quoted
    /// namespaced literals in order ARE the emitted names. Reading them back keeps the check honest:
    /// it compares the registry against the source of truth rather than a second hand-written list.
    pub fn names_in_module(&self) -> Vec<String> {
        // The namespace is derived from the first registered name, so an empty registry would make
        // this scan for the prefix `"."` and silently return nothing — which would then compare
        // equal to the empty registry and report agreement. Refuse instead.
        let first = self
            .names
            .first()
            .unwrap_or_else(|| panic!("stub `{}` has an empty counter registry", self.id));
        let namespace = format!("{}.", first.split('.').next().expect("namespaced counter name"));
        self.counters_align
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line.strip_prefix(&format!("print(\"{namespace}"))?;
                let name = rest.strip_suffix("\")")?;
                Some(format!("{namespace}{name}"))
            })
            .collect()
    }
}

pub const PG: Stub = Stub {
    id: "pg",
    c_source: include_str!("../fixtures/pkg_db_q2_postgres_stub.c"),
    counters_path: "pkg/db/testkit/pg.align",
    counters_align: PG_COUNTERS_ALIGN,
    names: &[
        "pg.connect_calls",
        "pg.finish_calls",
        "pg.encoding_calls",
        "pg.execute_calls",
        "pg.clear_calls",
        "pg.protocol_ok",
        "pg.protocol_error",
        "pg.nonblocking_calls",
        "pg.cancel_calls",
        "pg.cancel_socket_wait_calls",
        "pg.consume_calls",
        "pg.single_row_mode_calls",
        "pg.chunked_row_mode_calls",
        "pg.last_chunk_size",
        "pg.last_timeout",
        "pg.prepare_calls",
        "pg.execute_prepared_calls",
        "pg.control_calls",
        "pg.deallocate_calls",
        "pg.delivered_rows",
    ],
};

pub const SQLITE_Q4A: Stub = Stub {
    id: "sqlite-q4a",
    c_source: include_str!("../fixtures/pkg_db_q4a_sqlite_stub.c"),
    counters_path: "pkg/db/testkit/sqlite.align",
    counters_align: SQLITE_Q4A_COUNTERS_ALIGN,
    names: &[
        "sqlite.prepare_calls",
        "sqlite.bind_i64_calls",
        "sqlite.bind_text_calls",
        "sqlite.bind_blob_calls",
        "sqlite.reset_calls",
        "sqlite.clear_calls",
        "sqlite.finalize_calls",
        "sqlite.busy_timeout_calls",
        "sqlite.last_busy_timeout",
        "sqlite.protocol_ok",
    ],
};

pub const SQLITE_Q6: Stub = Stub {
    id: "sqlite-q6",
    c_source: include_str!("../fixtures/pkg_db_q6_sqlite_stub.c"),
    counters_path: "pkg/db/testkit/sqlite6.align",
    counters_align: SQLITE_Q6_COUNTERS_ALIGN,
    names: &[
        "sqlite6.prepare_calls",
        "sqlite6.step_calls",
        "sqlite6.delivered_rows",
        "sqlite6.finalize_calls",
        "sqlite6.protocol_ok",
    ],
};

/// Every stub, for the parameterized registry owner and for name lookup.
pub const ALL: &[&Stub] = &[&PG, &SQLITE_Q4A, &SQLITE_Q6];

/// C counters a stub exports that no Align dump module prints, and deliberately so.
///
/// The registry sweep compares each Align module against its registry, which by construction cannot
/// see a counter the C side exports but the Align side never reads. `align_pg_forbidden_after_status_calls`
/// is such a counter: the PostgreSQL status wave asserts on it from Rust through its own extern
/// block rather than through a dump, so it is invisible to the sweep. Listing it here records the
/// blind spot instead of leaving it implicit — a counter added to the C fixture and then forgotten
/// on the Align side looks identical to this from the sweep's point of view.
pub const C_ONLY_COUNTERS: &[&str] = &[
    "align_pg_forbidden_after_status_calls",
    "align_sqlite_pool_connect_calls",
    "align_sqlite_pool_close_calls",
];

/// Whether `name` is a counter some stub actually prints.
pub fn is_known_counter(name: &str) -> bool {
    ALL.iter().any(|stub| stub.names.contains(&name))
}

/// Every known counter name, for a failure message.
pub fn all_counter_names() -> Vec<&'static str> {
    ALL.iter().flat_map(|stub| stub.names.iter().copied()).collect()
}

// NOTE on `r##"`: each body contains `print("#db-counters-begin")`, whose `"#` would terminate an
// `r#"` string early.

const PG_COUNTERS_ALIGN: &str = r##"module pkg.db.testkit.pg

extern "C" {
  fn align_pg_reset()
  fn align_pg_connect_calls() -> i32
  fn align_pg_fail_connect_at(ordinal: i32)
  fn align_pg_finish_calls() -> i32
  fn align_pg_encoding_calls() -> i32
  fn align_pg_execute_calls() -> i32
  fn align_pg_clear_calls() -> i32
  fn align_pg_protocol_ok() -> i32
  fn align_pg_protocol_error() -> i32
  fn align_pg_nonblocking_calls() -> i32
  fn align_pg_cancel_calls() -> i32
  fn align_pg_cancel_socket_wait_calls() -> i32
  fn align_pg_consume_calls() -> i32
  fn align_pg_single_row_mode_calls() -> i32
  fn align_pg_chunked_row_mode_calls() -> i32
  fn align_pg_last_chunk_size() -> i32
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

pub fn fail_connect_at(ordinal: i32) {
  unsafe { align_pg_fail_connect_at(ordinal) }
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
    print("pg.cancel_socket_wait_calls")
    print(align_pg_cancel_socket_wait_calls())
    print("pg.consume_calls")
    print(align_pg_consume_calls())
    print("pg.single_row_mode_calls")
    print(align_pg_single_row_mode_calls())
    print("pg.chunked_row_mode_calls")
    print(align_pg_chunked_row_mode_calls())
    print("pg.last_chunk_size")
    print(align_pg_last_chunk_size())
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

const SQLITE_Q4A_COUNTERS_ALIGN: &str = r##"module pkg.db.testkit.sqlite

extern "C" {
  fn align_sqlite_q4a_reset()
  fn align_sqlite_q4a_prepare_calls() -> i32
  fn align_sqlite_q4a_bind_i64_calls() -> i32
  fn align_sqlite_q4a_bind_text_calls() -> i32
  fn align_sqlite_q4a_bind_blob_calls() -> i32
  fn align_sqlite_q4a_reset_calls() -> i32
  fn align_sqlite_q4a_clear_calls() -> i32
  fn align_sqlite_q4a_finalize_calls() -> i32
  fn align_sqlite_q4a_busy_timeout_calls() -> i32
  fn align_sqlite_q4a_last_busy_timeout() -> i32
  fn align_sqlite_q4a_protocol_ok() -> i32
}

pub fn reset() {
  unsafe { align_sqlite_q4a_reset() }
}

pub fn dump() {
  unsafe {
    print("#db-counters-begin")
    print("sqlite.prepare_calls")
    print(align_sqlite_q4a_prepare_calls())
    print("sqlite.bind_i64_calls")
    print(align_sqlite_q4a_bind_i64_calls())
    print("sqlite.bind_text_calls")
    print(align_sqlite_q4a_bind_text_calls())
    print("sqlite.bind_blob_calls")
    print(align_sqlite_q4a_bind_blob_calls())
    print("sqlite.reset_calls")
    print(align_sqlite_q4a_reset_calls())
    print("sqlite.clear_calls")
    print(align_sqlite_q4a_clear_calls())
    print("sqlite.finalize_calls")
    print(align_sqlite_q4a_finalize_calls())
    print("sqlite.busy_timeout_calls")
    print(align_sqlite_q4a_busy_timeout_calls())
    print("sqlite.last_busy_timeout")
    print(align_sqlite_q4a_last_busy_timeout())
    print("sqlite.protocol_ok")
    print(align_sqlite_q4a_protocol_ok())
    print("#db-counters-end")
  }
}
"##;

const SQLITE_Q6_COUNTERS_ALIGN: &str = r##"module pkg.db.testkit.sqlite6

extern "C" {
  fn align_sqlite_q6_reset()
  fn align_sqlite_q6_prepare_calls() -> i32
  fn align_sqlite_q6_step_calls() -> i32
  fn align_sqlite_q6_delivered_rows() -> i32
  fn align_sqlite_q6_finalize_calls() -> i32
  fn align_sqlite_q6_protocol_ok() -> i32
}

pub fn reset() {
  unsafe { align_sqlite_q6_reset() }
}

pub fn dump() {
  unsafe {
    print("#db-counters-begin")
    print("sqlite6.prepare_calls")
    print(align_sqlite_q6_prepare_calls())
    print("sqlite6.step_calls")
    print(align_sqlite_q6_step_calls())
    print("sqlite6.delivered_rows")
    print(align_sqlite_q6_delivered_rows())
    print("sqlite6.finalize_calls")
    print(align_sqlite_q6_finalize_calls())
    print("sqlite6.protocol_ok")
    print(align_sqlite_q6_protocol_ok())
    print("#db-counters-end")
  }
}
"##;
