//! Native database adapters for the explicit D11 migration workflow.

use crate::db_migrate::{
    HistoryRow, MigrationDriver, MigrationError, MigrationPolicy, MigrationReport, MigrationStatus,
    ScreenedCatalog, ScreenedMigration, StoredMigrationState, reconcile, screen_sqlite_catalog,
};
use crate::db_prepare::MigrationCatalog;
use crate::db_prepare_native::dynamic;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

fn fail(reason: impl Into<String>) -> MigrationError {
    MigrationError(reason.into())
}

fn native<T>(result: Result<T, crate::db_prepare::PrepareError>) -> Result<T, MigrationError> {
    result.map_err(|error| fail(error.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairAction {
    AcceptApplied,
    ClearDirty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationOperation<'a> {
    Migrate,
    Status,
    Check,
    Repair {
        version: u32,
        action: RepairAction,
        expected_checksum: &'a str,
    },
}

impl MigrationOperation<'_> {
    fn writes_database(self) -> bool {
        matches!(self, Self::Migrate | Self::Repair { .. })
    }

    fn requires_existing_database(self) -> bool {
        !matches!(self, Self::Migrate)
    }
}

const SQLITE_HISTORY_DDL: &str = r#"CREATE TABLE "__align_migrations_v1" (
  "format_version" INTEGER NOT NULL CHECK (typeof("format_version") = 'integer' AND "format_version" = 1),
  "version" INTEGER NOT NULL PRIMARY KEY CHECK (typeof("version") = 'integer' AND "version" BETWEEN 1 AND 9999),
  "filename" TEXT NOT NULL CHECK (typeof("filename") = 'text'),
  "checksum" TEXT NOT NULL CHECK (typeof("checksum") = 'text' AND length("checksum") = 32),
  "policy" INTEGER NOT NULL CHECK (typeof("policy") = 'integer' AND "policy" IN (0, 1)),
  "state" INTEGER NOT NULL CHECK (typeof("state") = 'integer' AND "state" IN (0, 1, 2)),
  "completed_statements" INTEGER NOT NULL CHECK (typeof("completed_statements") = 'integer' AND "completed_statements" BETWEEN 0 AND 4294967295)
)"#;

const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;
const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;
const SQLITE_OPEN_NOFOLLOW: c_int = 0x0100_0000;
const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_INTEGER: c_int = 1;
const SQLITE_TEXT: c_int = 3;
const MAX_HISTORY_ROWS: usize = 10_000;
const MAX_NATIVE_TEXT_BYTES: usize = 1_048_576;

type SqliteOpen =
    unsafe extern "C" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
type SqliteClose = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteErrmsg = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type SqliteComplete = unsafe extern "C" fn(*const c_char) -> c_int;
type SqliteExec = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
    *mut c_void,
    *mut *mut c_char,
) -> c_int;
type SqliteFree = unsafe extern "C" fn(*mut c_void);
type SqlitePrepare = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    c_int,
    *mut *mut c_void,
    *mut *const c_char,
) -> c_int;
type SqliteStep = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteFinalize = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteColumnCount = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteColumnType = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type SqliteColumnInt64 = unsafe extern "C" fn(*mut c_void, c_int) -> i64;
type SqliteColumnText = unsafe extern "C" fn(*mut c_void, c_int) -> *const u8;
type SqliteColumnBytes = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

struct SqliteApi {
    _library: dynamic::Library,
    open_v2: SqliteOpen,
    close_v2: SqliteClose,
    errmsg: SqliteErrmsg,
    complete: SqliteComplete,
    exec: SqliteExec,
    free: SqliteFree,
    prepare_v2: SqlitePrepare,
    step: SqliteStep,
    finalize: SqliteFinalize,
    column_count: SqliteColumnCount,
    column_type: SqliteColumnType,
    column_int64: SqliteColumnInt64,
    column_text: SqliteColumnText,
    column_bytes: SqliteColumnBytes,
}

impl SqliteApi {
    fn load() -> Result<Self, MigrationError> {
        #[cfg(target_os = "macos")]
        let candidates = [
            "/opt/homebrew/opt/sqlite/lib/libsqlite3.dylib",
            "/usr/local/opt/sqlite/lib/libsqlite3.dylib",
            "libsqlite3.dylib",
            "/usr/lib/libsqlite3.dylib",
        ];
        #[cfg(not(target_os = "macos"))]
        let candidates = ["libsqlite3.so.0", "libsqlite3.so", "libsqlite3.dylib", ""];
        let library = native(dynamic::Library::open(
            &candidates
                .iter()
                .copied()
                .filter(|candidate| !candidate.is_empty())
                .collect::<Vec<_>>(),
        ))?;
        // SAFETY: each symbol is assigned SQLite's documented C signature and `library` remains
        // owned by this function table for longer than every call.
        unsafe {
            Ok(Self {
                open_v2: native(library.symbol(b"sqlite3_open_v2\0"))?,
                close_v2: native(library.symbol(b"sqlite3_close_v2\0"))?,
                errmsg: native(library.symbol(b"sqlite3_errmsg\0"))?,
                complete: native(library.symbol(b"sqlite3_complete\0"))?,
                exec: native(library.symbol(b"sqlite3_exec\0"))?,
                free: native(library.symbol(b"sqlite3_free\0"))?,
                prepare_v2: native(library.symbol(b"sqlite3_prepare_v2\0"))?,
                step: native(library.symbol(b"sqlite3_step\0"))?,
                finalize: native(library.symbol(b"sqlite3_finalize\0"))?,
                column_count: native(library.symbol(b"sqlite3_column_count\0"))?,
                column_type: native(library.symbol(b"sqlite3_column_type\0"))?,
                column_int64: native(library.symbol(b"sqlite3_column_int64\0"))?,
                column_text: native(library.symbol(b"sqlite3_column_text\0"))?,
                column_bytes: native(library.symbol(b"sqlite3_column_bytes\0"))?,
                _library: library,
            })
        }
    }

    fn complete(&self, bytes: &[u8]) -> Result<bool, MigrationError> {
        let sql = CString::new(bytes).map_err(|_| fail("SQLite SQL contains U+0000"))?;
        // SAFETY: `sql` is a live NUL-terminated byte string for this synchronous call.
        Ok(unsafe { (self.complete)(sql.as_ptr()) } != 0)
    }
}

pub fn screen_sqlite_catalog_native(
    catalog: &MigrationCatalog,
) -> Result<ScreenedCatalog, MigrationError> {
    let api = SqliteApi::load()?;
    screen_sqlite_catalog(catalog, |bytes| api.complete(bytes))
}

#[derive(Debug)]
enum SqliteValue {
    Null,
    Integer(i64),
    Text(Vec<u8>),
    Other,
}

struct SqliteConnection {
    api: SqliteApi,
    database: *mut c_void,
}

impl SqliteConnection {
    fn open(path: &Path, flags: c_int) -> Result<Self, MigrationError> {
        let api = SqliteApi::load()?;
        let name = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| fail("SQLite database path contains U+0000"))?;
        let mut database = std::ptr::null_mut();
        // SAFETY: all inputs and output storage remain live for this synchronous open.
        let status = unsafe {
            (api.open_v2)(
                name.as_ptr(),
                &mut database,
                flags | SQLITE_OPEN_FULLMUTEX | SQLITE_OPEN_NOFOLLOW,
                std::ptr::null(),
            )
        };
        if status != SQLITE_OK || database.is_null() {
            let message = if database.is_null() {
                format!("SQLite open failed with status {status}")
            } else {
                sqlite_message(&api, database)
                    .unwrap_or_else(|| format!("SQLite open failed with status {status}"))
            };
            if !database.is_null() {
                // SAFETY: SQLite requires closing the partial handle returned by a failed open.
                unsafe { (api.close_v2)(database) };
            }
            return Err(fail(message));
        }
        Ok(Self { api, database })
    }

    fn execute(&self, bytes: &[u8], context: &str) -> Result<(), MigrationError> {
        let sql = CString::new(bytes).map_err(|_| fail(format!("{context} contains U+0000")))?;
        let mut native_error = std::ptr::null_mut();
        // SAFETY: connection and SQL stay live for the call; no callback is installed.
        let status = unsafe {
            (self.api.exec)(
                self.database,
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                &mut native_error,
            )
        };
        if status == SQLITE_OK {
            return Ok(());
        }
        let detail = if native_error.is_null() {
            sqlite_message(&self.api, self.database)
                .unwrap_or_else(|| format!("{context} failed with status {status}"))
        } else {
            // SAFETY: sqlite3_exec returns a NUL-terminated allocation owned by the caller.
            let bytes = unsafe { CStr::from_ptr(native_error) }.to_bytes();
            let copied = if bytes.len() > MAX_NATIVE_TEXT_BYTES {
                "SQLite native diagnostic exceeded the tool limit".to_string()
            } else {
                String::from_utf8_lossy(bytes).into_owned()
            };
            // SAFETY: the error allocation is released exactly once after copying.
            unsafe { (self.api.free)(native_error.cast()) };
            copied
        };
        Err(fail(format!("{context}: {detail}")))
    }

    fn query(
        &self,
        sql: &str,
        expected_fields: usize,
    ) -> Result<Vec<Vec<SqliteValue>>, MigrationError> {
        let sql = CString::new(sql).map_err(|_| fail("SQLite tool query contains U+0000"))?;
        let sql_len = c_int::try_from(sql.as_bytes().len())
            .map_err(|_| fail("SQLite tool query exceeds i32 length"))?;
        let mut statement = std::ptr::null_mut();
        let mut tail = std::ptr::null();
        // SAFETY: SQL, connection, and output storage are live for this prepare call.
        let status = unsafe {
            (self.api.prepare_v2)(
                self.database,
                sql.as_ptr(),
                sql_len,
                &mut statement,
                &mut tail,
            )
        };
        if status != SQLITE_OK || statement.is_null() {
            if !statement.is_null() {
                // SAFETY: a partial prepared statement is finalized exactly once.
                unsafe { (self.api.finalize)(statement) };
            }
            return Err(fail(
                sqlite_message(&self.api, self.database)
                    .unwrap_or_else(|| format!("SQLite query prepare failed with status {status}")),
            ));
        }
        let result = (|| {
            let fields = unsafe { (self.api.column_count)(statement) };
            if fields < 0 || usize::try_from(fields).ok() != Some(expected_fields) {
                return Err(fail("SQLite tool query returned an invalid shape"));
            }
            let mut rows = Vec::new();
            loop {
                match unsafe { (self.api.step)(statement) } {
                    SQLITE_DONE => break,
                    SQLITE_ROW => {
                        if rows.len() >= MAX_HISTORY_ROWS {
                            return Err(fail("SQLite tool query exceeded the row limit"));
                        }
                        let mut row = Vec::with_capacity(expected_fields);
                        for field in 0..fields {
                            let native_type = unsafe { (self.api.column_type)(statement, field) };
                            let value = match native_type {
                                5 => SqliteValue::Null,
                                SQLITE_INTEGER => SqliteValue::Integer(unsafe {
                                    (self.api.column_int64)(statement, field)
                                }),
                                SQLITE_TEXT => {
                                    let length =
                                        unsafe { (self.api.column_bytes)(statement, field) };
                                    if length < 0 {
                                        return Err(fail("SQLite returned a negative text length"));
                                    }
                                    let length = usize::try_from(length)
                                        .map_err(|_| fail("SQLite text length exceeds usize"))?;
                                    if length > MAX_NATIVE_TEXT_BYTES {
                                        return Err(fail(
                                            "SQLite text value exceeds the tool limit",
                                        ));
                                    }
                                    let pointer =
                                        unsafe { (self.api.column_text)(statement, field) };
                                    if pointer.is_null() && length != 0 {
                                        return Err(fail("SQLite returned null text storage"));
                                    }
                                    let bytes = if length == 0 {
                                        Vec::new()
                                    } else {
                                        // SAFETY: SQLite owns `length` live bytes until the next
                                        // step/finalize; copy them before either operation.
                                        unsafe { std::slice::from_raw_parts(pointer, length) }
                                            .to_vec()
                                    };
                                    SqliteValue::Text(bytes)
                                }
                                _ => SqliteValue::Other,
                            };
                            row.push(value);
                        }
                        rows.push(row);
                    }
                    status => {
                        return Err(fail(
                            sqlite_message(&self.api, self.database).unwrap_or_else(|| {
                                format!("SQLite query step failed with status {status}")
                            }),
                        ));
                    }
                }
            }
            Ok(rows)
        })();
        // SAFETY: the successfully prepared statement is finalized exactly once here.
        let finalized = unsafe { (self.api.finalize)(statement) };
        match (result, finalized) {
            (Err(error), _) => Err(error),
            (Ok(_), status) if status != SQLITE_OK => Err(fail(format!(
                "SQLite query finalize failed with status {status}"
            ))),
            (Ok(rows), _) => Ok(rows),
        }
    }

    fn close(&mut self) {
        if !self.database.is_null() {
            // SAFETY: this wrapper owns the open connection and closes it at most once.
            unsafe { (self.api.close_v2)(self.database) };
            self.database = std::ptr::null_mut();
        }
    }
}

impl Drop for SqliteConnection {
    fn drop(&mut self) {
        self.close();
    }
}

fn sqlite_message(api: &SqliteApi, database: *mut c_void) -> Option<String> {
    if database.is_null() {
        return None;
    }
    // SAFETY: SQLite returns a connection-owned NUL-terminated error message.
    let pointer = unsafe { (api.errmsg)(database) };
    if pointer.is_null() {
        None
    } else {
        let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
        Some(if bytes.len() > MAX_NATIVE_TEXT_BYTES {
            "SQLite native diagnostic exceeded the tool limit".to_string()
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        })
    }
}

#[cfg(unix)]
struct SqliteOperationLock {
    _file: File,
}

#[cfg(unix)]
impl SqliteOperationLock {
    fn acquire(database: &Path, exclusive: bool) -> Result<Self, MigrationError> {
        use std::os::unix::fs::OpenOptionsExt;

        let mut lock_name = database.as_os_str().to_os_string();
        lock_name.push(".align-migrate.lock");
        let lock_path = PathBuf::from(lock_name);
        #[cfg(target_os = "macos")]
        const O_NOFOLLOW: c_int = 0x0000_0100;
        #[cfg(target_os = "linux")]
        const O_NOFOLLOW: c_int = 0x0002_0000;
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        const O_NOFOLLOW: c_int = 0;
        let create = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(&lock_path);
        let file = match create {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(O_NOFOLLOW)
                .open(&lock_path)
                .map_err(|error| {
                    fail(format!(
                        "cannot open migration lock `{}`: {error}",
                        lock_path.display()
                    ))
                })?,
            Err(error) => {
                return Err(fail(format!(
                    "cannot create migration lock `{}`: {error}",
                    lock_path.display()
                )));
            }
        };
        let metadata = file.metadata().map_err(|error| {
            fail(format!(
                "cannot inspect migration lock `{}`: {error}",
                lock_path.display()
            ))
        })?;
        if !metadata.file_type().is_file() || metadata.len() != 0 {
            return Err(fail(format!(
                "migration lock `{}` must be an empty regular file",
                lock_path.display()
            )));
        }
        const LOCK_SH: c_int = 1;
        const LOCK_EX: c_int = 2;
        unsafe extern "C" {
            fn flock(fd: c_int, operation: c_int) -> c_int;
        }
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor stays owned by `file` until this lock guard is dropped.
        if unsafe { flock(file.as_raw_fd(), if exclusive { LOCK_EX } else { LOCK_SH }) } != 0 {
            return Err(fail(format!(
                "cannot acquire migration lock `{}`: {}",
                lock_path.display(),
                std::io::Error::last_os_error()
            )));
        }
        let locked_metadata = file.metadata().map_err(|error| {
            fail(format!(
                "cannot reinspect acquired migration lock `{}`: {error}",
                lock_path.display()
            ))
        })?;
        if !locked_metadata.file_type().is_file() || locked_metadata.len() != 0 {
            return Err(fail(format!(
                "acquired migration lock `{}` must remain an empty regular file",
                lock_path.display()
            )));
        }
        Ok(Self { _file: file })
    }
}

#[cfg(not(unix))]
struct SqliteOperationLock;

#[cfg(not(unix))]
impl SqliteOperationLock {
    fn acquire(_database: &Path, _exclusive: bool) -> Result<Self, MigrationError> {
        Err(fail(
            "SQLite migration locking is not supported on this host",
        ))
    }
}

fn sqlite_text(value: &SqliteValue, what: &str) -> Result<String, MigrationError> {
    let SqliteValue::Text(bytes) = value else {
        return Err(fail(format!(
            "migration history {what} is not stored as text"
        )));
    };
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|_| fail(format!("migration history {what} is not UTF-8")))
}

fn sqlite_u32(value: &SqliteValue, what: &str) -> Result<u32, MigrationError> {
    let SqliteValue::Integer(value) = value else {
        return Err(fail(format!(
            "migration history {what} is not stored as integer"
        )));
    };
    u32::try_from(*value).map_err(|_| fail(format!("migration history {what} is outside u32")))
}

fn validate_native_history_identity(
    format_version: u32,
    version: u32,
    filename: &str,
    checksum: &str,
) -> Result<(), MigrationError> {
    if format_version != 1 || !(1..=9999).contains(&version) {
        return Err(fail("migration history contains an invalid format/version"));
    }
    let bytes = filename.as_bytes();
    let stem = bytes
        .get(5..bytes.len().saturating_sub(4))
        .unwrap_or_default();
    if bytes.len() < 10
        || bytes.get(4) != Some(&b'_')
        || !filename.ends_with(".sql")
        || bytes
            .get(..4)
            .is_none_or(|prefix| !prefix.iter().all(u8::is_ascii_digit))
        || stem.is_empty()
        || !stem[0].is_ascii_lowercase()
        || !stem[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        || filename
            .get(..4)
            .and_then(|prefix| prefix.parse::<u32>().ok())
            != Some(version)
    {
        return Err(fail(format!(
            "migration history version {version:04} has an invalid filename"
        )));
    }
    if checksum.len() != 32
        || !checksum
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(fail(format!(
            "migration history version {version:04} has an invalid checksum"
        )));
    }
    Ok(())
}

fn validate_sqlite_schema(connection: &SqliteConnection) -> Result<bool, MigrationError> {
    let rows = connection.query(
        "SELECT type,name,tbl_name,sql FROM main.sqlite_schema WHERE tbl_name='__align_migrations_v1' ORDER BY type,name",
        4,
    )?;
    let temp = connection.query(
        "SELECT type,name,tbl_name,sql FROM sqlite_temp_schema WHERE tbl_name='__align_migrations_v1' ORDER BY type,name",
        4,
    )?;
    if !temp.is_empty() {
        return Err(fail(
            "SQLite migration history has a temporary shadow or attached object",
        ));
    }
    let inbound_foreign_keys = connection.query(
        "SELECT fk.\"table\" FROM main.sqlite_schema AS object JOIN pragma_foreign_key_list(object.name) AS fk WHERE object.type='table' AND fk.\"table\"='__align_migrations_v1' LIMIT 1",
        1,
    )?;
    if !inbound_foreign_keys.is_empty() {
        return Err(fail("SQLite migration history has an inbound foreign key"));
    }
    if rows.is_empty() {
        return Ok(false);
    }
    if rows.len() != 1 {
        return Err(fail(
            "SQLite migration history has unexpected attached objects",
        ));
    }
    let row = &rows[0];
    if sqlite_text(&row[0], "object type")? != "table"
        || sqlite_text(&row[1], "object name")? != "__align_migrations_v1"
        || sqlite_text(&row[2], "object table")? != "__align_migrations_v1"
        || sqlite_text(&row[3], "creation SQL")? != SQLITE_HISTORY_DDL
    {
        return Err(fail(
            "SQLite migration history schema does not match the canonical DDL",
        ));
    }
    Ok(true)
}

fn read_sqlite_history(connection: &SqliteConnection) -> Result<Vec<HistoryRow>, MigrationError> {
    let rows = connection.query(
        "SELECT format_version,version,filename,checksum,policy,state,completed_statements FROM main.__align_migrations_v1 ORDER BY version",
        7,
    )?;
    rows.into_iter()
        .map(|row| {
            let [
                format_version,
                version,
                filename,
                checksum,
                policy,
                state,
                completed,
            ] = row.as_slice()
            else {
                return Err(fail(
                    "SQLite migration history returned an invalid row shape",
                ));
            };
            let format_version = sqlite_u32(format_version, "format_version")?;
            let version = sqlite_u32(version, "version")?;
            let filename = sqlite_text(filename, "filename")?;
            let checksum = sqlite_text(checksum, "checksum")?;
            validate_native_history_identity(format_version, version, &filename, &checksum)?;
            let policy_value = sqlite_u32(policy, "policy")?;
            let state_value = sqlite_u32(state, "state")?;
            let completed_statements = sqlite_u32(completed, "completed_statements")?;
            Ok(HistoryRow {
                format_version,
                version,
                filename,
                checksum,
                policy: u8::try_from(policy_value)
                    .ok()
                    .and_then(MigrationPolicy::from_tag)
                    .ok_or_else(|| fail("migration history policy is invalid"))?,
                state: u8::try_from(state_value)
                    .ok()
                    .and_then(StoredMigrationState::from_tag)
                    .ok_or_else(|| fail("migration history state is invalid"))?,
                completed_statements,
            })
        })
        .collect()
}

fn inspect_sqlite(
    connection: &SqliteConnection,
    catalog: &ScreenedCatalog,
    history_required: bool,
) -> Result<MigrationReport, MigrationError> {
    let exists = validate_sqlite_schema(connection)?;
    if !exists {
        if history_required {
            return Err(fail("SQLite migration history is missing"));
        }
        return reconcile(MigrationDriver::Sqlite, catalog, Vec::new());
    }
    reconcile(
        MigrationDriver::Sqlite,
        catalog,
        read_sqlite_history(connection)?,
    )
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn history_insert_sql(
    migration: &ScreenedMigration,
    state: StoredMigrationState,
    completed: u32,
) -> String {
    format!(
        "INSERT INTO main.__align_migrations_v1 (format_version,version,filename,checksum,policy,state,completed_statements) VALUES (1,{}, {}, {},{},{},{})",
        migration.version,
        sql_literal(&migration.filename),
        sql_literal(&migration.checksum),
        migration.policy.tag(),
        state.tag(),
        completed,
    )
}

fn stored_history_insert_sql(table: &str, row: &HistoryRow) -> String {
    format!(
        "INSERT INTO {table} (format_version,version,filename,checksum,policy,state,completed_statements) VALUES ({},{},{},{},{},{},{})",
        row.format_version,
        row.version,
        sql_literal(&row.filename),
        sql_literal(&row.checksum),
        row.policy.tag(),
        row.state.tag(),
        row.completed_statements,
    )
}

fn rollback(connection: &SqliteConnection) {
    let _ = connection.execute(b"ROLLBACK", "SQLite migration rollback");
}

fn sqlite_snapshot(
    connection: &SqliteConnection,
    catalog: &ScreenedCatalog,
    required: bool,
) -> Result<MigrationReport, MigrationError> {
    connection.execute(b"BEGIN", "SQLite history snapshot")?;
    let report = inspect_sqlite(connection, catalog, required);
    match report {
        Ok(report) => {
            if let Err(error) = connection.execute(b"COMMIT", "SQLite history snapshot commit") {
                rollback(connection);
                Err(error)
            } else {
                Ok(report)
            }
        }
        Err(error) => {
            rollback(connection);
            Err(error)
        }
    }
}

fn ensure_prefix(report: &MigrationReport) -> Result<(), MigrationError> {
    if report.can_migrate() {
        Ok(())
    } else {
        Err(fail(
            "migration history is not an exact Applied prefix of the catalog",
        ))
    }
}

fn ensure_ready(report: &MigrationReport, version: u32) -> Result<(), MigrationError> {
    for row in &report.rows {
        let expected = if row.version < version {
            MigrationStatus::Applied
        } else {
            MigrationStatus::Pending
        };
        if row.status != expected {
            return Err(fail(format!(
                "migration history is not ready to apply version {version:04}"
            )));
        }
    }
    if report
        .rows
        .iter()
        .any(|row| row.version == version && row.status == MigrationStatus::Pending)
    {
        Ok(())
    } else {
        Err(fail(format!(
            "migration version {version:04} is not pending"
        )))
    }
}

#[derive(Clone)]
enum SqliteCommitExpectation {
    Bootstrap,
    Applied(u32),
    Applying {
        version: u32,
        history: Vec<HistoryRow>,
    },
    Failed(u32),
    Cleared(u32),
}

fn sqlite_reconcile_commit(
    connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
    expectation: SqliteCommitExpectation,
    commit_error: MigrationError,
) -> Result<(), MigrationError> {
    connection.close();
    *connection = SqliteConnection::open(path, SQLITE_OPEN_READWRITE)?;
    if matches!(&expectation, SqliteCommitExpectation::Bootstrap) {
        connection.execute(b"BEGIN IMMEDIATE", "SQLite bootstrap reconciliation")?;
        let exists = validate_sqlite_schema(connection).map_err(|error| {
            rollback(connection);
            fail(format!(
                "SQLite bootstrap commit outcome is unknown after `{commit_error}`; reconciliation failed: {error}"
            ))
        })?;
        if !exists {
            rollback(connection);
            return Err(fail(format!(
                "SQLite bootstrap commit failed and reconciliation proved it was not applied: {commit_error}"
            )));
        }
        let report = inspect_sqlite(connection, catalog, true).map_err(|error| {
            rollback(connection);
            fail(format!(
                "SQLite bootstrap commit outcome is unknown after `{commit_error}`; reconciliation failed: {error}"
            ))
        })?;
        connection.execute(b"COMMIT", "SQLite bootstrap reconciliation commit")?;
        return ensure_prefix(&report);
    }
    let report = sqlite_snapshot(connection, catalog, true).map_err(|error| {
        fail(format!(
            "SQLite commit outcome is unknown after `{commit_error}`; reconciliation failed: {error}"
        ))
    })?;
    let status = |version| {
        report
            .rows
            .iter()
            .find(|row| row.version == version)
            .map(|row| row.status)
    };
    let observed_history = report
        .rows
        .iter()
        .filter_map(|row| row.history.clone())
        .collect::<Vec<_>>();
    let reconciled = match &expectation {
        SqliteCommitExpectation::Bootstrap => report.can_migrate(),
        SqliteCommitExpectation::Applied(version) => {
            status(*version) == Some(MigrationStatus::Applied)
        }
        SqliteCommitExpectation::Applying { history, .. } => {
            observed_history.as_slice() == history.as_slice()
        }
        SqliteCommitExpectation::Failed(version) => {
            status(*version) == Some(MigrationStatus::DirtyFailed)
        }
        SqliteCommitExpectation::Cleared(version) => {
            status(*version) == Some(MigrationStatus::Pending)
        }
    };
    if reconciled {
        return Ok(());
    }
    let known_absence = match &expectation {
        SqliteCommitExpectation::Bootstrap => false,
        SqliteCommitExpectation::Applied(version) | SqliteCommitExpectation::Failed(version) => {
            status(*version) == Some(MigrationStatus::Pending)
        }
        SqliteCommitExpectation::Applying { version, .. } => {
            status(*version) == Some(MigrationStatus::Pending)
        }
        SqliteCommitExpectation::Cleared(version) => matches!(
            status(*version),
            Some(MigrationStatus::DirtyApplying | MigrationStatus::DirtyFailed)
        ),
    };
    if known_absence {
        Err(fail(format!(
            "SQLite commit failed and reconciliation proved the requested change was not applied: {commit_error}"
        )))
    } else {
        Err(fail(format!(
            "SQLite commit outcome is unknown after reconciliation: {commit_error}"
        )))
    }
}

fn sqlite_commit(
    connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
    expectation: SqliteCommitExpectation,
) -> Result<(), MigrationError> {
    match connection.execute(b"COMMIT", "SQLite migration commit") {
        Ok(()) => Ok(()),
        Err(error) => sqlite_reconcile_commit(connection, path, catalog, expectation, error),
    }
}

fn sqlite_bootstrap(
    connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
) -> Result<(), MigrationError> {
    connection.execute(b"BEGIN IMMEDIATE", "SQLite migration write transaction")?;
    let result = (|| {
        if validate_sqlite_schema(connection)? {
            return Err(fail("SQLite migration history appeared during bootstrap"));
        }
        connection.execute(
            SQLITE_HISTORY_DDL.as_bytes(),
            "SQLite migration history bootstrap",
        )?;
        let report = inspect_sqlite(connection, catalog, true)?;
        ensure_prefix(&report)
    })();
    match result {
        Ok(()) => sqlite_commit(
            connection,
            path,
            catalog,
            SqliteCommitExpectation::Bootstrap,
        ),
        Err(error) => {
            rollback(connection);
            Err(error)
        }
    }
}

fn sqlite_required(
    connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
    migration: &ScreenedMigration,
) -> Result<(), MigrationError> {
    connection.execute(b"BEGIN IMMEDIATE", "SQLite migration write transaction")?;
    let result = (|| {
        let before = inspect_sqlite(connection, catalog, true)?;
        ensure_ready(&before, migration.version)?;
        let row = before
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .ok_or_else(|| fail("current migration disappeared during reconciliation"))?;
        if row.status != MigrationStatus::Pending {
            return Err(fail(format!(
                "migration {:04} is not pending",
                migration.version
            )));
        }
        connection.execute(
            &migration.bytes,
            &format!("SQLite migration {:04}", migration.version),
        )?;
        let after_sql = inspect_sqlite(connection, catalog, true)?;
        ensure_ready(&after_sql, migration.version)?;
        connection.execute(
            history_insert_sql(
                migration,
                StoredMigrationState::Applied,
                migration.statement_count,
            )
            .as_bytes(),
            "SQLite migration history insert",
        )?;
        let after_insert = inspect_sqlite(connection, catalog, true)?;
        if after_insert
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .map(|row| row.status)
            != Some(MigrationStatus::Applied)
        {
            return Err(fail(
                "SQLite migration history insert did not reread as Applied",
            ));
        }
        Ok(())
    })();
    match result {
        Ok(()) => sqlite_commit(
            connection,
            path,
            catalog,
            SqliteCommitExpectation::Applied(migration.version),
        ),
        Err(error) => {
            rollback(connection);
            Err(error)
        }
    }
}

fn sqlite_forbidden(
    history_connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
    migration: &ScreenedMigration,
) -> Result<(), MigrationError> {
    history_connection.execute(b"BEGIN IMMEDIATE", "SQLite migration write transaction")?;
    let mut expected_history = None;
    let applying = (|| {
        let before = inspect_sqlite(history_connection, catalog, true)?;
        ensure_ready(&before, migration.version)?;
        let row = before
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .ok_or_else(|| fail("current migration disappeared during reconciliation"))?;
        if row.status != MigrationStatus::Pending {
            return Err(fail(format!(
                "migration {:04} is not pending",
                migration.version
            )));
        }
        let mut history = before
            .rows
            .iter()
            .filter_map(|row| row.history.clone())
            .collect::<Vec<_>>();
        history.push(HistoryRow {
            format_version: 1,
            version: migration.version,
            filename: migration.filename.clone(),
            checksum: migration.checksum.clone(),
            policy: migration.policy,
            state: StoredMigrationState::Applying,
            completed_statements: 0,
        });
        history.sort_by_key(|row| row.version);
        expected_history = Some(history);
        history_connection.execute(
            history_insert_sql(migration, StoredMigrationState::Applying, 0).as_bytes(),
            "SQLite Applying history insert",
        )
    })();
    if let Err(error) = applying {
        rollback(history_connection);
        return Err(error);
    }
    let applying_history = expected_history
        .ok_or_else(|| fail("SQLite Applying snapshot was not captured before publication"))?;
    sqlite_commit(
        history_connection,
        path,
        catalog,
        SqliteCommitExpectation::Applying {
            version: migration.version,
            history: applying_history.clone(),
        },
    )?;

    let native_result = SqliteConnection::open(path, SQLITE_OPEN_READWRITE).and_then(|worker| {
        worker.execute(
            &migration.bytes,
            &format!("SQLite forbidden migration {:04}", migration.version),
        )
    });
    let final_state = if native_result.is_ok() {
        StoredMigrationState::Applied
    } else {
        StoredMigrationState::Failed
    };
    let completed = u32::from(final_state == StoredMigrationState::Applied);
    history_connection.execute(b"BEGIN IMMEDIATE", "SQLite migration write transaction")?;
    let publication = (|| {
        let restored_missing = !validate_sqlite_schema(history_connection)?;
        let history_unchanged = if restored_missing {
            false
        } else {
            let observed = read_sqlite_history(history_connection)?;
            reconcile(MigrationDriver::Sqlite, catalog, observed.clone())?;
            observed == applying_history
        };
        if !history_unchanged {
            if restored_missing {
                history_connection.execute(
                    SQLITE_HISTORY_DDL.as_bytes(),
                    "SQLite forbidden missing-history restore",
                )?;
            }
            history_connection.execute(
                b"DELETE FROM main.__align_migrations_v1",
                "SQLite forbidden history snapshot restore",
            )?;
            for row in &applying_history {
                history_connection.execute(
                    stored_history_insert_sql("main.__align_migrations_v1", row).as_bytes(),
                    "SQLite forbidden history snapshot restore",
                )?;
            }
            let restored = inspect_sqlite(history_connection, catalog, true)?;
            let expected = restored
                .rows
                .iter()
                .find(|row| row.version == migration.version);
            if expected.map(|row| row.status) != Some(MigrationStatus::DirtyApplying) {
                return Err(fail(
                    "SQLite forbidden migration lost its Applying history row",
                ));
            }
            return Ok(true);
        }
        history_connection.execute(
            format!(
                "UPDATE main.__align_migrations_v1 SET state={},completed_statements={} WHERE version={} AND state=0 AND completed_statements=0",
                final_state.tag(), completed, migration.version
            )
            .as_bytes(),
            "SQLite forbidden history update",
        )?;
        let after = inspect_sqlite(history_connection, catalog, true)?;
        let expected_status = if final_state == StoredMigrationState::Applied {
            MigrationStatus::Applied
        } else {
            MigrationStatus::DirtyFailed
        };
        if after
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .map(|row| row.status)
            != Some(expected_status)
        {
            return Err(fail(
                "SQLite forbidden migration final state did not reread exactly",
            ));
        }
        Ok(false)
    })();
    let publication = match publication {
        Ok(history_changed) => {
            let committed = sqlite_commit(
                history_connection,
                path,
                catalog,
                if history_changed {
                    SqliteCommitExpectation::Applying {
                        version: migration.version,
                        history: applying_history.clone(),
                    }
                } else if final_state == StoredMigrationState::Applied {
                    SqliteCommitExpectation::Applied(migration.version)
                } else {
                    SqliteCommitExpectation::Failed(migration.version)
                },
            );
            committed.map(|()| history_changed)
        }
        Err(error) => {
            rollback(history_connection);
            Err(error)
        }
    };
    match (native_result, publication) {
        (Err(native_error), Ok(true)) => Err(fail(format!(
            "{native_error}; SQLite migration history changed and the exact Applying snapshot was restored"
        ))),
        (Err(native_error), Ok(false)) => Err(native_error),
        (Err(native_error), Err(record_error)) => Err(fail(format!(
            "{native_error}; additionally failed to record Failed state: {record_error}"
        ))),
        (Ok(()), Ok(true)) => Err(fail(
            "SQLite migration history changed; the exact Applying snapshot was restored",
        )),
        (Ok(()), Ok(false)) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn sqlite_migrate(
    connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
) -> Result<MigrationReport, MigrationError> {
    connection.execute(b"BEGIN IMMEDIATE", "SQLite migration bootstrap check")?;
    let exists = validate_sqlite_schema(connection);
    rollback(connection);
    if !exists? {
        sqlite_bootstrap(connection, path, catalog)?;
    }
    let initial = sqlite_snapshot(connection, catalog, true)?;
    ensure_prefix(&initial)?;
    let pending = initial
        .rows
        .iter()
        .filter(|row| row.status == MigrationStatus::Pending)
        .filter_map(|row| row.catalog.clone())
        .collect::<Vec<_>>();
    for migration in &pending {
        match migration.policy {
            MigrationPolicy::Required => sqlite_required(connection, path, catalog, migration)?,
            MigrationPolicy::Forbidden => sqlite_forbidden(connection, path, catalog, migration)?,
        }
    }
    let final_report = sqlite_snapshot(connection, catalog, true)?;
    if !final_report.is_current() {
        return Err(fail(
            "SQLite migrate did not produce an exact Applied catalog",
        ));
    }
    Ok(final_report)
}

fn sqlite_repair(
    connection: &mut SqliteConnection,
    path: &Path,
    catalog: &ScreenedCatalog,
    version: u32,
    action: RepairAction,
    expected_checksum: &str,
) -> Result<MigrationReport, MigrationError> {
    connection.execute(b"BEGIN IMMEDIATE", "SQLite migration write transaction")?;
    let result = (|| {
        let report = inspect_sqlite(connection, catalog, true)?;
        let row = report
            .rows
            .iter()
            .find(|row| row.version == version && row.catalog.is_some())
            .ok_or_else(|| {
                fail(format!(
                    "repair version {version:04} is not in the current catalog"
                ))
            })?;
        let current = row.catalog.as_ref().expect("checked above");
        let history = row
            .history
            .as_ref()
            .ok_or_else(|| fail(format!("repair version {version:04} has no history row")))?;
        if current.checksum != expected_checksum || history.checksum != expected_checksum {
            return Err(fail(format!(
                "repair version {version:04} checksum does not match"
            )));
        }
        if !matches!(
            row.status,
            MigrationStatus::DirtyApplying | MigrationStatus::DirtyFailed
        ) {
            return Err(fail(format!("repair version {version:04} is not dirty")));
        }
        match action {
            RepairAction::AcceptApplied => connection.execute(
                format!(
                    "UPDATE main.__align_migrations_v1 SET state=1,completed_statements={} WHERE version={} AND state IN (0,2) AND completed_statements=0",
                    current.statement_count, version
                )
                .as_bytes(),
                "SQLite repair accept-applied",
            )?,
            RepairAction::ClearDirty => connection.execute(
                format!(
                    "DELETE FROM main.__align_migrations_v1 WHERE version={} AND state IN (0,2) AND completed_statements=0",
                    version
                )
                .as_bytes(),
                "SQLite repair clear-dirty",
            )?,
        }
        let after = inspect_sqlite(connection, catalog, true)?;
        let expected_status = match action {
            RepairAction::AcceptApplied => MigrationStatus::Applied,
            RepairAction::ClearDirty => MigrationStatus::Pending,
        };
        if after
            .rows
            .iter()
            .find(|row| row.version == version)
            .map(|row| row.status)
            != Some(expected_status)
        {
            return Err(fail(
                "SQLite repair did not produce the requested exact state",
            ));
        }
        Ok(after)
    })();
    match result {
        Ok(report) => {
            sqlite_commit(
                connection,
                path,
                catalog,
                match action {
                    RepairAction::AcceptApplied => SqliteCommitExpectation::Applied(version),
                    RepairAction::ClearDirty => SqliteCommitExpectation::Cleared(version),
                },
            )?;
            Ok(report)
        }
        Err(error) => {
            rollback(connection);
            Err(error)
        }
    }
}

pub fn run_sqlite_migration(
    path: &Path,
    operation: MigrationOperation<'_>,
    catalog: &ScreenedCatalog,
) -> Result<MigrationReport, MigrationError> {
    let metadata = std::fs::symlink_metadata(path);
    match metadata {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(fail(format!(
                "SQLite target `{}` is a symlink",
                path.display()
            )));
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(fail(format!(
                "SQLite target `{}` is not a regular file",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && !operation.requires_existing_database() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(fail(format!(
                "SQLite target `{}` does not exist",
                path.display()
            )));
        }
        Err(error) => {
            return Err(fail(format!(
                "cannot inspect SQLite target `{}`: {error}",
                path.display()
            )));
        }
    }
    let _lock = SqliteOperationLock::acquire(path, operation.writes_database())?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let native_parent = std::fs::canonicalize(parent).map_err(|error| {
        fail(format!(
            "cannot resolve SQLite target parent `{}`: {error}",
            parent.display()
        ))
    })?;
    let filename = path
        .file_name()
        .ok_or_else(|| fail("SQLite target has no filename"))?;
    // SQLite's unix VFS applies SQLITE_OPEN_NOFOLLOW to path resolution, including symlinked
    // ancestors on macOS. Resolve only the already-inspected parent and append the final component;
    // the native flag still rejects a raced final symlink without changing target identity.
    let native_path = native_parent.join(filename);
    let flags = if operation.writes_database() {
        SQLITE_OPEN_READWRITE
            | if matches!(operation, MigrationOperation::Migrate) {
                SQLITE_OPEN_CREATE
            } else {
                0
            }
    } else {
        SQLITE_OPEN_READONLY
    };
    let mut connection = SqliteConnection::open(&native_path, flags)?;
    match operation {
        MigrationOperation::Migrate => sqlite_migrate(&mut connection, &native_path, catalog),
        MigrationOperation::Status => sqlite_snapshot(&connection, catalog, false),
        MigrationOperation::Check => sqlite_snapshot(&connection, catalog, false),
        MigrationOperation::Repair {
            version,
            action,
            expected_checksum,
        } => sqlite_repair(
            &mut connection,
            &native_path,
            catalog,
            version,
            action,
            expected_checksum,
        ),
    }
}

pub fn validate_postgres_migration_environment() -> Result<(), MigrationError> {
    native(crate::db_prepare_native::reject_ambient_postgres_migration_environment())
}

pub fn validate_postgres_migration_url(url: &str) -> Result<(), MigrationError> {
    native(crate::db_prepare_native::validate_complete_postgres_migration_url(url))
}

const POSTGRES_HISTORY_DDL: &str = r#"CREATE TABLE "align_internal"."migrations_v1" (
  "format_version" integer NOT NULL CHECK ("format_version" = 1),
  "version" integer NOT NULL PRIMARY KEY CHECK ("version" BETWEEN 1 AND 9999),
  "filename" text NOT NULL,
  "checksum" text NOT NULL CHECK (length("checksum") = 32),
  "policy" integer NOT NULL CHECK ("policy" IN (0, 1)),
  "state" integer NOT NULL CHECK ("state" IN (0, 1, 2)),
  "completed_statements" bigint NOT NULL CHECK ("completed_statements" BETWEEN 0 AND 4294967295)
)"#;

type PqConnectParams =
    unsafe extern "C" fn(*const *const c_char, *const *const c_char, c_int) -> *mut c_void;
type PqFinish = unsafe extern "C" fn(*mut c_void);
type PqStatus = unsafe extern "C" fn(*const c_void) -> c_int;
type PqClientEncoding = unsafe extern "C" fn(*const c_void) -> c_int;
type PqExec = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type PqClear = unsafe extern "C" fn(*mut c_void);
type PqResultStatus = unsafe extern "C" fn(*const c_void) -> c_int;
type PqResultErrorMessage = unsafe extern "C" fn(*const c_void) -> *const c_char;
type PqResultErrorField = unsafe extern "C" fn(*const c_void, c_int) -> *const c_char;
type PqCount = unsafe extern "C" fn(*const c_void) -> c_int;
type PqGetIsNull = unsafe extern "C" fn(*const c_void, c_int, c_int) -> c_int;
type PqGetValue = unsafe extern "C" fn(*const c_void, c_int, c_int) -> *mut c_char;
type PqGetLength = unsafe extern "C" fn(*const c_void, c_int, c_int) -> c_int;

struct PostgresApi {
    _library: dynamic::Library,
    connectdb_params: PqConnectParams,
    finish: PqFinish,
    status: PqStatus,
    client_encoding: PqClientEncoding,
    exec: PqExec,
    clear: PqClear,
    result_status: PqResultStatus,
    result_error_message: PqResultErrorMessage,
    result_error_field: PqResultErrorField,
    ntuples: PqCount,
    nfields: PqCount,
    getisnull: PqGetIsNull,
    getvalue: PqGetValue,
    getlength: PqGetLength,
}

impl PostgresApi {
    fn load() -> Result<Self, MigrationError> {
        #[cfg(target_os = "macos")]
        let candidates = [
            "/opt/homebrew/opt/libpq/lib/libpq.5.dylib",
            "/usr/local/opt/libpq/lib/libpq.5.dylib",
            "libpq.5.dylib",
            "libpq.dylib",
        ];
        #[cfg(not(target_os = "macos"))]
        let candidates = ["libpq.so.5", "libpq.so", "libpq.5.dylib", ""];
        let library = native(dynamic::Library::open(
            &candidates
                .iter()
                .copied()
                .filter(|candidate| !candidate.is_empty())
                .collect::<Vec<_>>(),
        ))?;
        // SAFETY: each symbol is assigned libpq's documented C signature and the library remains
        // owned by this table for the lifetime of every connection/result call.
        unsafe {
            Ok(Self {
                connectdb_params: native(library.symbol(b"PQconnectdbParams\0"))?,
                finish: native(library.symbol(b"PQfinish\0"))?,
                status: native(library.symbol(b"PQstatus\0"))?,
                client_encoding: native(library.symbol(b"PQclientEncoding\0"))?,
                exec: native(library.symbol(b"PQexec\0"))?,
                clear: native(library.symbol(b"PQclear\0"))?,
                result_status: native(library.symbol(b"PQresultStatus\0"))?,
                result_error_message: native(library.symbol(b"PQresultErrorMessage\0"))?,
                result_error_field: native(library.symbol(b"PQresultErrorField\0"))?,
                ntuples: native(library.symbol(b"PQntuples\0"))?,
                nfields: native(library.symbol(b"PQnfields\0"))?,
                getisnull: native(library.symbol(b"PQgetisnull\0"))?,
                getvalue: native(library.symbol(b"PQgetvalue\0"))?,
                getlength: native(library.symbol(b"PQgetlength\0"))?,
                _library: library,
            })
        }
    }
}

#[derive(Debug)]
struct PostgresCommandError {
    message: String,
    sqlstate: Option<String>,
}

fn postgres_history_is_missing(error: &PostgresCommandError) -> bool {
    matches!(error.sqlstate.as_deref(), Some("42P01" | "3F000"))
}

impl From<PostgresCommandError> for MigrationError {
    fn from(error: PostgresCommandError) -> Self {
        fail(error.message)
    }
}

struct PostgresConnection {
    api: PostgresApi,
    connection: *mut c_void,
}

impl PostgresConnection {
    fn open(url: &str) -> Result<Self, MigrationError> {
        let api = PostgresApi::load()?;
        let url = CString::new(url).map_err(|_| fail("PostgreSQL URL contains U+0000"))?;
        let dbname = CString::new("dbname").expect("literal");
        let client_encoding = CString::new("client_encoding").expect("literal");
        let options = CString::new("options").expect("literal");
        let utf8 = CString::new("UTF8").expect("literal");
        let no_options = CString::new(" ").expect("literal");
        let keywords = [
            dbname.as_ptr(),
            client_encoding.as_ptr(),
            options.as_ptr(),
            std::ptr::null(),
        ];
        let values = [
            url.as_ptr(),
            utf8.as_ptr(),
            no_options.as_ptr(),
            std::ptr::null(),
        ];
        // SAFETY: arrays and C strings are live for this synchronous libpq connection call.
        let connection = unsafe { (api.connectdb_params)(keywords.as_ptr(), values.as_ptr(), 1) };
        if connection.is_null() {
            return Err(fail("libpq returned a null PostgreSQL connection"));
        }
        if unsafe { (api.status)(connection) } != 0 {
            // SAFETY: the failed connection handle is released exactly once without copying its
            // potentially secret-bearing native diagnostic.
            unsafe { (api.finish)(connection) };
            return Err(fail("PostgreSQL connection failed"));
        }
        if unsafe { (api.client_encoding)(connection) } != 6 {
            unsafe { (api.finish)(connection) };
            return Err(fail("PostgreSQL client encoding is not UTF-8"));
        }
        Ok(Self { api, connection })
    }

    fn execute_raw(&self, bytes: &[u8], context: &str) -> Result<(), PostgresCommandError> {
        let sql = CString::new(bytes).map_err(|_| PostgresCommandError {
            message: format!("{context} contains U+0000"),
            sqlstate: None,
        })?;
        // SAFETY: connection and SQL remain live for the synchronous libpq call.
        let result = unsafe { (self.api.exec)(self.connection, sql.as_ptr()) };
        if result.is_null() {
            return Err(PostgresCommandError {
                message: format!("libpq returned a null {context} result"),
                sqlstate: None,
            });
        }
        let status = unsafe { (self.api.result_status)(result) };
        let decoded = if matches!(status, 1 | 2) {
            Ok(())
        } else {
            const PG_DIAG_SQLSTATE: c_int = b'C' as c_int;
            let message_pointer = unsafe { (self.api.result_error_message)(result) };
            let message = pq_owned_text(message_pointer)
                .unwrap_or_else(|| format!("{context} failed with status {status}"));
            let state_pointer = unsafe { (self.api.result_error_field)(result, PG_DIAG_SQLSTATE) };
            Err(PostgresCommandError {
                message: format!("{context}: {message}"),
                sqlstate: pq_owned_text(state_pointer),
            })
        };
        // SAFETY: the non-null result is cleared after all referenced text is copied.
        unsafe { (self.api.clear)(result) };
        decoded
    }

    fn execute(&self, bytes: &[u8], context: &str) -> Result<(), MigrationError> {
        self.execute_raw(bytes, context).map_err(Into::into)
    }

    fn query(
        &self,
        sql: &str,
        expected_fields: usize,
    ) -> Result<Vec<Vec<Option<String>>>, MigrationError> {
        let sql = CString::new(sql).map_err(|_| fail("PostgreSQL tool query contains U+0000"))?;
        let result = unsafe { (self.api.exec)(self.connection, sql.as_ptr()) };
        if result.is_null() {
            return Err(fail("libpq returned a null PostgreSQL query result"));
        }
        let decoded = (|| {
            let status = unsafe { (self.api.result_status)(result) };
            if status != 2 {
                let message = pq_owned_text(unsafe { (self.api.result_error_message)(result) })
                    .unwrap_or_else(|| format!("PostgreSQL query failed with status {status}"));
                return Err(fail(message));
            }
            let rows = unsafe { (self.api.ntuples)(result) };
            let fields = unsafe { (self.api.nfields)(result) };
            if rows < 0 || fields < 0 || usize::try_from(fields).ok() != Some(expected_fields) {
                return Err(fail("PostgreSQL tool query returned an invalid shape"));
            }
            let capacity =
                usize::try_from(rows).map_err(|_| fail("PostgreSQL row count exceeds usize"))?;
            if capacity > MAX_HISTORY_ROWS {
                return Err(fail("PostgreSQL tool query exceeded the row limit"));
            }
            let mut output = Vec::with_capacity(capacity);
            for row in 0..rows {
                let mut values = Vec::with_capacity(expected_fields);
                for field in 0..fields {
                    if unsafe { (self.api.getisnull)(result, row, field) } != 0 {
                        values.push(None);
                    } else {
                        let pointer = unsafe { (self.api.getvalue)(result, row, field) };
                        let length = unsafe { (self.api.getlength)(result, row, field) };
                        if length < 0 {
                            return Err(fail("PostgreSQL query returned a negative value length"));
                        }
                        let length = usize::try_from(length)
                            .map_err(|_| fail("PostgreSQL value length exceeds usize"))?;
                        if length > MAX_NATIVE_TEXT_BYTES {
                            return Err(fail("PostgreSQL text value exceeds the tool limit"));
                        }
                        if pointer.is_null() && length != 0 {
                            return Err(fail("PostgreSQL query returned null text storage"));
                        }
                        let bytes = if length == 0 {
                            &[][..]
                        } else {
                            // SAFETY: libpq owns `length` bytes until PQclear; validation and copy
                            // complete before that result owner is released.
                            unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) }
                        };
                        values.push(Some(
                            std::str::from_utf8(bytes)
                                .map_err(|_| fail("PostgreSQL query value is not UTF-8"))?
                                .to_string(),
                        ));
                    }
                }
                output.push(values);
            }
            Ok(output)
        })();
        unsafe { (self.api.clear)(result) };
        decoded
    }

    fn advisory_lock(&self, exclusive: bool) -> Result<(), MigrationError> {
        let function = if exclusive {
            "pg_advisory_lock"
        } else {
            "pg_advisory_lock_shared"
        };
        let rows = self.query(
            &format!("SELECT pg_catalog.{function}(1095518535,1296647985)"),
            1,
        )?;
        if rows.len() != 1 {
            return Err(fail("PostgreSQL advisory lock returned an invalid shape"));
        }
        Ok(())
    }

    fn close(&mut self) {
        if !self.connection.is_null() {
            // SAFETY: this wrapper owns the live PGconn and releases it at most once.
            unsafe { (self.api.finish)(self.connection) };
            self.connection = std::ptr::null_mut();
        }
    }
}

impl Drop for PostgresConnection {
    fn drop(&mut self) {
        self.close();
    }
}

fn pq_owned_text(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
    if bytes.len() > MAX_NATIVE_TEXT_BYTES {
        return Some("PostgreSQL native diagnostic exceeded the tool limit".to_string());
    }
    std::str::from_utf8(bytes).ok().map(str::to_string)
}

fn postgres_begin_and_lock(
    connection: &PostgresConnection,
    write: bool,
) -> Result<(), PostgresCommandError> {
    connection.execute_raw(
        b"BEGIN ISOLATION LEVEL READ COMMITTED",
        "PostgreSQL history transaction",
    )?;
    let mode = if write {
        "ACCESS EXCLUSIVE"
    } else {
        "SHARE ROW EXCLUSIVE"
    };
    connection.execute_raw(
        format!("LOCK TABLE \"align_internal\".\"migrations_v1\" IN {mode} MODE").as_bytes(),
        "PostgreSQL history table lock",
    )
}

fn postgres_rollback(connection: &PostgresConnection) {
    let _ = connection.execute(b"ROLLBACK", "PostgreSQL migration rollback");
}

fn postgres_schema_inventory_sql() -> &'static str {
    r#"WITH target AS (
  SELECT c.oid AS table_oid,c.relkind,c.relpersistence,c.relowner,c.relispartition,c.relrowsecurity,c.relforcerowsecurity,c.relreplident,n.oid AS schema_oid,n.nspowner
  FROM pg_catalog.pg_namespace n
  JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname='migrations_v1'
  WHERE n.nspname='align_internal'
), columns AS (
  SELECT a.attrelid,
         pg_catalog.string_agg(a.attname || ':' || pg_catalog.format_type(a.atttypid,a.atttypmod) || ':' || a.attnotnull::text || ':' || a.attidentity || ':' || a.attgenerated || ':' || (ad.oid IS NOT NULL)::text, ',' ORDER BY a.attnum) AS signature,
         pg_catalog.count(*) AS count
  FROM pg_catalog.pg_attribute a
  LEFT JOIN pg_catalog.pg_attrdef ad ON ad.adrelid=a.attrelid AND ad.adnum=a.attnum
  JOIN target t ON t.table_oid=a.attrelid
  WHERE a.attnum>0 AND NOT a.attisdropped
  GROUP BY a.attrelid
), constraints AS (
  SELECT conrelid,
         pg_catalog.string_agg(contype || ':' || pg_catalog.pg_get_constraintdef(oid,false), E'\n' ORDER BY contype,pg_catalog.pg_get_constraintdef(oid,false)) AS signature,
         pg_catalog.count(*) AS count,
         pg_catalog.bool_and(convalidated AND NOT condeferrable AND NOT condeferred) AS immediate
  FROM pg_catalog.pg_constraint
  JOIN target t ON t.table_oid=conrelid
  GROUP BY conrelid
), indexes AS (
  SELECT i.indrelid,pg_catalog.count(*) AS count,
         pg_catalog.bool_and(i.indisprimary AND i.indisvalid AND i.indisready AND i.indimmediate AND i.indisunique AND NOT i.indisexclusion AND NOT i.indnullsnotdistinct AND i.indnatts=1 AND i.indnkeyatts=1 AND i.indkey[0]=2 AND i.indoption[0]=0 AND i.indcollation[0]=0 AND am.amname='btree' AND x.indpred IS NULL AND x.indexprs IS NULL AND i.indclass[0]=(SELECT op.oid FROM pg_catalog.pg_opclass op WHERE op.opcmethod=am.oid AND op.opcintype='integer'::pg_catalog.regtype AND op.opcdefault)) AS exact
  FROM pg_catalog.pg_index i
  JOIN target t ON t.table_oid=i.indrelid
  JOIN pg_catalog.pg_class ic ON ic.oid=i.indexrelid
  JOIN pg_catalog.pg_am am ON am.oid=ic.relam
  JOIN pg_catalog.pg_index x ON x.indexrelid=i.indexrelid
  GROUP BY i.indrelid
)
SELECT t.relkind,t.relpersistence,(t.relowner=current_user::pg_catalog.regrole)::text,t.relispartition::text,t.relrowsecurity::text,t.relforcerowsecurity::text,
       (t.nspowner=current_user::pg_catalog.regrole)::text,c.signature,c.count::text,k.signature,k.count::text,k.immediate::text,i.count::text,i.exact::text,
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.pg_inherits h WHERE h.inhrelid=t.table_oid OR h.inhparent=t.table_oid),
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.pg_trigger g WHERE g.tgrelid=t.table_oid),
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.pg_rewrite r WHERE r.ev_class=t.table_oid),
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.pg_policy p WHERE p.polrelid=t.table_oid),
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.aclexplode(COALESCE((SELECT relacl FROM pg_catalog.pg_class WHERE oid=t.table_oid),pg_catalog.acldefault('r',t.relowner))) a WHERE a.grantee<>t.relowner AND a.privilege_type IS NOT NULL),
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.pg_attribute ca CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(ca.attacl,ARRAY[]::pg_catalog.aclitem[])) a WHERE ca.attrelid=t.table_oid AND ca.attnum>0 AND NOT ca.attisdropped AND a.grantee<>t.relowner AND a.privilege_type IS NOT NULL),
       (SELECT am.amname FROM pg_catalog.pg_am am WHERE am.oid=(SELECT relam FROM pg_catalog.pg_class WHERE oid=t.table_oid)),
       t.relreplident,
       (SELECT pg_catalog.count(*)::text FROM pg_catalog.pg_publication_rel pr WHERE pr.prrelid=t.table_oid)
FROM target t JOIN columns c ON c.attrelid=t.table_oid JOIN constraints k ON k.conrelid=t.table_oid JOIN indexes i ON i.indrelid=t.table_oid"#
}

fn validate_postgres_schema(connection: &PostgresConnection) -> Result<(), MigrationError> {
    let rows = connection.query(postgres_schema_inventory_sql(), 23)?;
    let [row] = rows.as_slice() else {
        return Err(fail(
            "PostgreSQL migration history schema has an invalid object inventory",
        ));
    };
    let value = |index: usize, what: &str| {
        row.get(index)
            .and_then(Option::as_deref)
            .ok_or_else(|| fail(format!("PostgreSQL migration history {what} is NULL")))
    };
    let columns = "format_version:integer:true:::false,version:integer:true:::false,filename:text:true:::false,checksum:text:true:::false,policy:integer:true:::false,state:integer:true:::false,completed_statements:bigint:true:::false";
    let constraints = value(9, "constraint inventory")?;
    let required_checks = [
        "CHECK ((format_version = 1))",
        "CHECK (((version >= 1) AND (version <= 9999)))",
        "CHECK ((length(checksum) = 32))",
        "CHECK ((policy = ANY (ARRAY[0, 1])))",
        "CHECK ((state = ANY (ARRAY[0, 1, 2])))",
        "CHECK (((completed_statements >= 0) AND (completed_statements <= '4294967295'::bigint)))",
        "PRIMARY KEY (version)",
    ];
    let mut constraint_lines = constraints
        .lines()
        .map(|line| {
            canonical_postgres_constraint(line.split_once(':').map_or(line, |(_, value)| value))
        })
        .collect::<Vec<_>>();
    constraint_lines.sort();
    let mut required_checks = required_checks
        .iter()
        .map(|value| canonical_postgres_constraint(value))
        .collect::<Vec<_>>();
    required_checks.sort();
    if value(0, "relation kind")? != "r"
        || value(1, "persistence")? != "p"
        || value(2, "owner")? != "true"
        || value(3, "partition flag")? != "false"
        || value(4, "row-security flag")? != "false"
        || value(5, "forced-row-security flag")? != "false"
        || value(6, "schema owner")? != "true"
        || value(7, "column inventory")? != columns
        || value(8, "column count")? != "7"
        || value(10, "constraint count")? != "7"
        || value(11, "constraint timing")? != "true"
        || constraint_lines != required_checks
        || value(12, "index count")? != "1"
        || value(13, "primary index")? != "true"
        || value(14, "inheritance inventory")? != "0"
        || value(15, "trigger inventory")? != "0"
        || value(16, "rewrite-rule inventory")? != "0"
        || value(17, "policy inventory")? != "0"
        || value(18, "table ACL inventory")? != "0"
        || value(19, "column ACL inventory")? != "0"
        || value(20, "table access method")? != "heap"
        || value(21, "replica identity")? != "d"
        || value(22, "publication inventory")? != "0"
    {
        return Err(fail(
            "PostgreSQL migration history schema does not match the canonical contract",
        ));
    }
    Ok(())
}

fn canonical_postgres_constraint(value: &str) -> String {
    value
        .replace("::bigint", "")
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'(' | b')' | b'\'' | b'"'))
        .map(char::from)
        .collect()
}

fn parse_postgres_u32(value: Option<&String>, what: &str) -> Result<u32, MigrationError> {
    value
        .ok_or_else(|| fail(format!("PostgreSQL migration history {what} is NULL")))?
        .parse::<u32>()
        .map_err(|_| {
            fail(format!(
                "PostgreSQL migration history {what} is outside u32"
            ))
        })
}

fn read_postgres_history(
    connection: &PostgresConnection,
) -> Result<Vec<HistoryRow>, MigrationError> {
    connection
        .query(
            "SELECT format_version::text,version::text,filename,checksum,policy::text,state::text,completed_statements::text FROM \"align_internal\".\"migrations_v1\" ORDER BY version",
            7,
        )?
        .into_iter()
        .map(|row| {
            let [format_version, version, filename, checksum, policy, state, completed] = row.as_slice() else {
                return Err(fail("PostgreSQL migration history returned an invalid row shape"));
            };
            let format_version = parse_postgres_u32(format_version.as_ref(), "format_version")?;
            let version = parse_postgres_u32(version.as_ref(), "version")?;
            let filename = filename
                .clone()
                .ok_or_else(|| fail("PostgreSQL migration history filename is NULL"))?;
            let checksum = checksum
                .clone()
                .ok_or_else(|| fail("PostgreSQL migration history checksum is NULL"))?;
            validate_native_history_identity(format_version, version, &filename, &checksum)?;
            let policy = parse_postgres_u32(policy.as_ref(), "policy")?;
            let state = parse_postgres_u32(state.as_ref(), "state")?;
            let completed_statements =
                parse_postgres_u32(completed.as_ref(), "completed_statements")?;
            Ok(HistoryRow {
                format_version,
                version,
                filename,
                checksum,
                policy: u8::try_from(policy).ok().and_then(MigrationPolicy::from_tag).ok_or_else(|| fail("PostgreSQL migration history policy is invalid"))?,
                state: u8::try_from(state).ok().and_then(StoredMigrationState::from_tag).ok_or_else(|| fail("PostgreSQL migration history state is invalid"))?,
                completed_statements,
            })
        })
        .collect()
}

fn inspect_postgres_locked(
    connection: &PostgresConnection,
    catalog: &ScreenedCatalog,
) -> Result<MigrationReport, MigrationError> {
    validate_postgres_schema(connection)?;
    reconcile(
        MigrationDriver::Postgres,
        catalog,
        read_postgres_history(connection)?,
    )
}

fn postgres_absent_snapshot(
    connection: &PostgresConnection,
    catalog: &ScreenedCatalog,
) -> Result<MigrationReport, MigrationError> {
    connection.execute(
        b"BEGIN ISOLATION LEVEL READ COMMITTED",
        "PostgreSQL absent-history snapshot",
    )?;
    let result = (|| {
        let rows = connection.query(
            "SELECT (pg_catalog.to_regnamespace('align_internal') IS NOT NULL)::text,(pg_catalog.to_regclass('align_internal.migrations_v1') IS NOT NULL)::text",
            2,
        )?;
        let [row] = rows.as_slice() else {
            return Err(fail(
                "PostgreSQL absent-history query returned an invalid shape",
            ));
        };
        match (row[0].as_deref(), row[1].as_deref()) {
            (Some("false"), Some("false")) => {
                reconcile(MigrationDriver::Postgres, catalog, Vec::new())
            }
            _ => Err(fail(
                "PostgreSQL migration history has exactly one or malformed owned object",
            )),
        }
    })();
    match result {
        Ok(report) => {
            connection.execute(b"COMMIT", "PostgreSQL absent-history snapshot commit")?;
            Ok(report)
        }
        Err(error) => {
            postgres_rollback(connection);
            Err(error)
        }
    }
}

fn postgres_inspect_phase(
    connection: &PostgresConnection,
    catalog: &ScreenedCatalog,
    write: bool,
    absent_allowed: bool,
) -> Result<MigrationReport, MigrationError> {
    match postgres_begin_and_lock(connection, write) {
        Ok(()) => {}
        Err(error) if postgres_history_is_missing(&error) => {
            postgres_rollback(connection);
            if absent_allowed {
                return postgres_absent_snapshot(connection, catalog);
            }
            return Err(fail("PostgreSQL migration history is missing"));
        }
        Err(error) => {
            postgres_rollback(connection);
            return Err(error.into());
        }
    }
    let report = inspect_postgres_locked(connection, catalog);
    match report {
        Ok(report) => {
            connection.execute(b"COMMIT", "PostgreSQL history transaction commit")?;
            Ok(report)
        }
        Err(error) => {
            postgres_rollback(connection);
            Err(error)
        }
    }
}

#[derive(Clone)]
enum PostgresCommitExpectation {
    Bootstrap,
    Applied(u32),
    Applying {
        version: u32,
        history: Vec<HistoryRow>,
    },
    Failed(u32),
    Cleared(u32),
}

fn postgres_reconcile_commit(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
    expectation: PostgresCommitExpectation,
    commit_error: MigrationError,
) -> Result<(), MigrationError> {
    connection.close();
    *connection = PostgresConnection::open(url).map_err(|error| {
        fail(format!(
            "PostgreSQL commit outcome is unknown after `{commit_error}`; reconnect failed: {error}"
        ))
    })?;
    connection.advisory_lock(true).map_err(|error| {
        fail(format!(
            "PostgreSQL commit outcome is unknown after `{commit_error}`; relock failed: {error}"
        ))
    })?;
    if matches!(&expectation, PostgresCommitExpectation::Bootstrap) {
        match postgres_begin_and_lock(connection, true) {
            Ok(()) => {}
            Err(error) if postgres_history_is_missing(&error) => {
                postgres_rollback(connection);
                return Err(fail(format!(
                    "PostgreSQL bootstrap commit failed and reconciliation proved it was not applied: {commit_error}"
                )));
            }
            Err(error) => {
                postgres_rollback(connection);
                return Err(fail(format!(
                    "PostgreSQL bootstrap commit outcome is unknown after `{commit_error}`; relock failed: {}",
                    error.message
                )));
            }
        }
        let report = inspect_postgres_locked(connection, catalog).map_err(|error| {
            postgres_rollback(connection);
            fail(format!(
                "PostgreSQL bootstrap commit outcome is unknown after `{commit_error}`; reconciliation failed: {error}"
            ))
        })?;
        connection.execute(b"COMMIT", "PostgreSQL bootstrap reconciliation commit")?;
        return ensure_prefix(&report);
    }
    let report = postgres_inspect_phase(connection, catalog, true, false).map_err(|error| {
        fail(format!(
            "PostgreSQL commit outcome is unknown after `{commit_error}`; reconciliation failed: {error}"
        ))
    })?;
    let status = |version| {
        report
            .rows
            .iter()
            .find(|row| row.version == version)
            .map(|row| row.status)
    };
    let observed_history = report
        .rows
        .iter()
        .filter_map(|row| row.history.clone())
        .collect::<Vec<_>>();
    let reconciled = match &expectation {
        PostgresCommitExpectation::Bootstrap => report.can_migrate(),
        PostgresCommitExpectation::Applied(version) => {
            status(*version) == Some(MigrationStatus::Applied)
        }
        PostgresCommitExpectation::Applying { history, .. } => {
            observed_history.as_slice() == history.as_slice()
        }
        PostgresCommitExpectation::Failed(version) => {
            status(*version) == Some(MigrationStatus::DirtyFailed)
        }
        PostgresCommitExpectation::Cleared(version) => {
            status(*version) == Some(MigrationStatus::Pending)
        }
    };
    if reconciled {
        return Ok(());
    }
    let known_absence = match &expectation {
        PostgresCommitExpectation::Bootstrap => false,
        PostgresCommitExpectation::Applied(version)
        | PostgresCommitExpectation::Failed(version) => {
            status(*version) == Some(MigrationStatus::Pending)
        }
        PostgresCommitExpectation::Applying { version, .. } => {
            status(*version) == Some(MigrationStatus::Pending)
        }
        PostgresCommitExpectation::Cleared(version) => matches!(
            status(*version),
            Some(MigrationStatus::DirtyApplying | MigrationStatus::DirtyFailed)
        ),
    };
    if known_absence {
        Err(fail(format!(
            "PostgreSQL commit failed and reconciliation proved the requested change was not applied: {commit_error}"
        )))
    } else {
        Err(fail(format!(
            "PostgreSQL commit outcome is unknown after reconciliation: {commit_error}"
        )))
    }
}

fn postgres_commit(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
    expectation: PostgresCommitExpectation,
    context: &str,
) -> Result<(), MigrationError> {
    match connection.execute(b"COMMIT", context) {
        Ok(()) => Ok(()),
        Err(error) => postgres_reconcile_commit(connection, url, catalog, expectation, error),
    }
}

fn postgres_bootstrap(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
) -> Result<(), MigrationError> {
    connection.execute(
        b"BEGIN ISOLATION LEVEL READ COMMITTED",
        "PostgreSQL history bootstrap",
    )?;
    let result = (|| {
        connection.execute(
            b"CREATE SCHEMA \"align_internal\"",
            "PostgreSQL history schema bootstrap",
        )?;
        connection.execute(
            POSTGRES_HISTORY_DDL.as_bytes(),
            "PostgreSQL history table bootstrap",
        )?;
        let report = inspect_postgres_locked(connection, catalog)?;
        ensure_prefix(&report)
    })();
    match result {
        Ok(()) => postgres_commit(
            connection,
            url,
            catalog,
            PostgresCommitExpectation::Bootstrap,
            "PostgreSQL history bootstrap commit",
        ),
        Err(error) => {
            postgres_rollback(connection);
            Err(error)
        }
    }
}

fn postgres_required(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
    migration: &ScreenedMigration,
) -> Result<(), MigrationError> {
    if let Err(error) = postgres_begin_and_lock(connection, true) {
        postgres_rollback(connection);
        return Err(error.into());
    }
    let result = (|| {
        let before = inspect_postgres_locked(connection, catalog)?;
        ensure_ready(&before, migration.version)?;
        if before
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .map(|row| row.status)
            != Some(MigrationStatus::Pending)
        {
            return Err(fail(format!(
                "migration {:04} is not pending",
                migration.version
            )));
        }
        connection.execute(
            &migration.bytes,
            &format!("PostgreSQL migration {:04}", migration.version),
        )?;
        let after_sql = inspect_postgres_locked(connection, catalog)?;
        ensure_ready(&after_sql, migration.version)?;
        connection.execute(
            history_insert_sql(
                migration,
                StoredMigrationState::Applied,
                migration.statement_count,
            )
            .replace(
                "main.__align_migrations_v1",
                "\"align_internal\".\"migrations_v1\"",
            )
            .as_bytes(),
            "PostgreSQL migration history insert",
        )?;
        let after = inspect_postgres_locked(connection, catalog)?;
        if after
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .map(|row| row.status)
            != Some(MigrationStatus::Applied)
        {
            return Err(fail(
                "PostgreSQL migration history insert did not reread as Applied",
            ));
        }
        Ok(())
    })();
    match result {
        Ok(()) => postgres_commit(
            connection,
            url,
            catalog,
            PostgresCommitExpectation::Applied(migration.version),
            "PostgreSQL migration commit",
        ),
        Err(error) => {
            postgres_rollback(connection);
            Err(error)
        }
    }
}

fn postgres_restore_missing_forbidden_history(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
    history: &[HistoryRow],
    version: u32,
) -> Result<(), MigrationError> {
    connection.execute(
        b"BEGIN ISOLATION LEVEL READ COMMITTED",
        "PostgreSQL forbidden missing-history restore",
    )?;
    let restored = (|| {
        let rows = connection.query(
            "SELECT (pg_catalog.to_regnamespace('align_internal') IS NOT NULL)::text,(pg_catalog.to_regclass('align_internal.migrations_v1') IS NOT NULL)::text",
            2,
        )?;
        let [row] = rows.as_slice() else {
            return Err(fail(
                "PostgreSQL forbidden history restore returned an invalid shape",
            ));
        };
        match (row[0].as_deref(), row[1].as_deref()) {
            (Some("false"), Some("false")) => connection.execute(
                b"CREATE SCHEMA \"align_internal\"",
                "PostgreSQL forbidden history schema restore",
            )?,
            (Some("true"), Some("false")) => {
                let owner = connection.query(
                    "SELECT (nspowner=current_user::pg_catalog.regrole)::text FROM pg_catalog.pg_namespace WHERE nspname='align_internal'",
                    1,
                )?;
                if owner.as_slice() != [vec![Some("true".to_string())]] {
                    return Err(fail(
                        "PostgreSQL forbidden history schema is not owned by the current role",
                    ));
                }
            }
            _ => {
                return Err(fail(
                    "PostgreSQL forbidden history restore raced an owned-object replacement",
                ));
            }
        }
        connection.execute(
            POSTGRES_HISTORY_DDL.as_bytes(),
            "PostgreSQL forbidden history table restore",
        )?;
        validate_postgres_schema(connection)?;
        for row in history {
            connection.execute(
                stored_history_insert_sql("\"align_internal\".\"migrations_v1\"", row).as_bytes(),
                "PostgreSQL forbidden history snapshot restore",
            )?;
        }
        let report = inspect_postgres_locked(connection, catalog)?;
        if report
            .rows
            .iter()
            .find(|row| row.version == version)
            .map(|row| row.status)
            != Some(MigrationStatus::DirtyApplying)
        {
            return Err(fail(
                "PostgreSQL restored forbidden history did not reread as Applying",
            ));
        }
        Ok(())
    })();
    match restored {
        Ok(()) => postgres_commit(
            connection,
            url,
            catalog,
            PostgresCommitExpectation::Applying {
                version,
                history: history.to_vec(),
            },
            "PostgreSQL forbidden history restore commit",
        ),
        Err(error) => {
            postgres_rollback(connection);
            Err(error)
        }
    }
}

fn postgres_forbidden(
    history_connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
    migration: &ScreenedMigration,
) -> Result<(), MigrationError> {
    if let Err(error) = postgres_begin_and_lock(history_connection, true) {
        postgres_rollback(history_connection);
        return Err(error.into());
    }
    let mut expected_history = None;
    let applying = (|| {
        let before = inspect_postgres_locked(history_connection, catalog)?;
        ensure_ready(&before, migration.version)?;
        if before
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .map(|row| row.status)
            != Some(MigrationStatus::Pending)
        {
            return Err(fail(format!(
                "migration {:04} is not pending",
                migration.version
            )));
        }
        let mut history = before
            .rows
            .iter()
            .filter_map(|row| row.history.clone())
            .collect::<Vec<_>>();
        history.push(HistoryRow {
            format_version: 1,
            version: migration.version,
            filename: migration.filename.clone(),
            checksum: migration.checksum.clone(),
            policy: migration.policy,
            state: StoredMigrationState::Applying,
            completed_statements: 0,
        });
        history.sort_by_key(|row| row.version);
        expected_history = Some(history);
        history_connection.execute(
            history_insert_sql(migration, StoredMigrationState::Applying, 0)
                .replace(
                    "main.__align_migrations_v1",
                    "\"align_internal\".\"migrations_v1\"",
                )
                .as_bytes(),
            "PostgreSQL Applying history insert",
        )
    })();
    if let Err(error) = applying {
        postgres_rollback(history_connection);
        return Err(error);
    }
    let applying_history = expected_history
        .ok_or_else(|| fail("PostgreSQL Applying snapshot was not captured before publication"))?;
    postgres_commit(
        history_connection,
        url,
        catalog,
        PostgresCommitExpectation::Applying {
            version: migration.version,
            history: applying_history.clone(),
        },
        "PostgreSQL Applying history commit",
    )?;

    let native_result = PostgresConnection::open(url).and_then(|worker| {
        worker.execute(
            &migration.bytes,
            &format!("PostgreSQL forbidden migration {:04}", migration.version),
        )
    });
    let final_state = if native_result.is_ok() {
        StoredMigrationState::Applied
    } else {
        StoredMigrationState::Failed
    };
    let completed = u32::from(final_state == StoredMigrationState::Applied);
    if let Err(error) = postgres_begin_and_lock(history_connection, true) {
        postgres_rollback(history_connection);
        if postgres_history_is_missing(&error) {
            postgres_restore_missing_forbidden_history(
                history_connection,
                url,
                catalog,
                &applying_history,
                migration.version,
            )?;
            return match native_result {
                Ok(()) => Err(fail(
                    "PostgreSQL forbidden SQL removed migration history; the exact Applying snapshot was restored",
                )),
                Err(native_error) => Err(fail(format!(
                    "{native_error}; migration history was removed and the exact Applying snapshot was restored"
                ))),
            };
        }
        return Err(error.into());
    }
    let publication = (|| {
        validate_postgres_schema(history_connection)?;
        let observed = read_postgres_history(history_connection)?;
        reconcile(MigrationDriver::Postgres, catalog, observed.clone())?;
        let history_unchanged = observed == applying_history;
        if !history_unchanged {
            history_connection.execute(
                b"DELETE FROM \"align_internal\".\"migrations_v1\"",
                "PostgreSQL forbidden history snapshot restore",
            )?;
            for row in &applying_history {
                history_connection.execute(
                    stored_history_insert_sql("\"align_internal\".\"migrations_v1\"", row)
                        .as_bytes(),
                    "PostgreSQL forbidden history snapshot restore",
                )?;
            }
            let restored = inspect_postgres_locked(history_connection, catalog)?;
            if restored
                .rows
                .iter()
                .find(|row| row.version == migration.version)
                .map(|row| row.status)
                != Some(MigrationStatus::DirtyApplying)
            {
                return Err(fail(
                    "PostgreSQL forbidden migration lost its Applying history row",
                ));
            }
            return Ok(true);
        }
        history_connection.execute(
            format!(
                "UPDATE \"align_internal\".\"migrations_v1\" SET state={},completed_statements={} WHERE version={} AND state=0 AND completed_statements=0",
                final_state.tag(), completed, migration.version
            ).as_bytes(),
            "PostgreSQL forbidden history update",
        )?;
        let after = inspect_postgres_locked(history_connection, catalog)?;
        let expected = if final_state == StoredMigrationState::Applied {
            MigrationStatus::Applied
        } else {
            MigrationStatus::DirtyFailed
        };
        if after
            .rows
            .iter()
            .find(|row| row.version == migration.version)
            .map(|row| row.status)
            != Some(expected)
        {
            return Err(fail(
                "PostgreSQL forbidden migration final state did not reread exactly",
            ));
        }
        Ok(false)
    })();
    let publication = match publication {
        Ok(history_changed) => postgres_commit(
            history_connection,
            url,
            catalog,
            if history_changed {
                PostgresCommitExpectation::Applying {
                    version: migration.version,
                    history: applying_history.clone(),
                }
            } else if final_state == StoredMigrationState::Applied {
                PostgresCommitExpectation::Applied(migration.version)
            } else {
                PostgresCommitExpectation::Failed(migration.version)
            },
            "PostgreSQL forbidden history commit",
        )
        .map(|()| history_changed),
        Err(error) => {
            postgres_rollback(history_connection);
            Err(error)
        }
    };
    match (native_result, publication) {
        (Err(native_error), Ok(true)) => Err(fail(format!(
            "{native_error}; PostgreSQL migration history changed and the exact Applying snapshot was restored"
        ))),
        (Err(native_error), Ok(false)) => Err(native_error),
        (Err(native_error), Err(record_error)) => Err(fail(format!(
            "{native_error}; additionally failed to record Failed state: {record_error}"
        ))),
        (Ok(()), Ok(true)) => Err(fail(
            "PostgreSQL migration history changed; the exact Applying snapshot was restored",
        )),
        (Ok(()), Ok(false)) => Ok(()),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn postgres_migrate(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
) -> Result<MigrationReport, MigrationError> {
    let initial = match postgres_begin_and_lock(connection, true) {
        Ok(()) => {
            let result = inspect_postgres_locked(connection, catalog);
            match result {
                Ok(report) => {
                    connection.execute(b"COMMIT", "PostgreSQL initial history commit")?;
                    report
                }
                Err(error) => {
                    postgres_rollback(connection);
                    return Err(error);
                }
            }
        }
        Err(error) if postgres_history_is_missing(&error) => {
            postgres_rollback(connection);
            postgres_bootstrap(connection, url, catalog)?;
            postgres_inspect_phase(connection, catalog, true, false)?
        }
        Err(error) => {
            postgres_rollback(connection);
            return Err(error.into());
        }
    };
    ensure_prefix(&initial)?;
    let pending = initial
        .rows
        .iter()
        .filter(|row| row.status == MigrationStatus::Pending)
        .filter_map(|row| row.catalog.clone())
        .collect::<Vec<_>>();
    for migration in &pending {
        match migration.policy {
            MigrationPolicy::Required => postgres_required(connection, url, catalog, migration)?,
            MigrationPolicy::Forbidden => postgres_forbidden(connection, url, catalog, migration)?,
        }
    }
    let report = postgres_inspect_phase(connection, catalog, true, false)?;
    if !report.is_current() {
        return Err(fail(
            "PostgreSQL migrate did not produce an exact Applied catalog",
        ));
    }
    Ok(report)
}

fn postgres_repair(
    connection: &mut PostgresConnection,
    url: &str,
    catalog: &ScreenedCatalog,
    version: u32,
    action: RepairAction,
    expected_checksum: &str,
) -> Result<MigrationReport, MigrationError> {
    if let Err(error) = postgres_begin_and_lock(connection, true) {
        postgres_rollback(connection);
        return if postgres_history_is_missing(&error) {
            Err(fail("PostgreSQL migration history is missing"))
        } else {
            Err(error.into())
        };
    }
    let result = (|| {
        let report = inspect_postgres_locked(connection, catalog)?;
        let row = report
            .rows
            .iter()
            .find(|row| row.version == version && row.catalog.is_some())
            .ok_or_else(|| {
                fail(format!(
                    "repair version {version:04} is not in the current catalog"
                ))
            })?;
        let current = row.catalog.as_ref().expect("checked above");
        let history = row
            .history
            .as_ref()
            .ok_or_else(|| fail(format!("repair version {version:04} has no history row")))?;
        if current.checksum != expected_checksum || history.checksum != expected_checksum {
            return Err(fail(format!(
                "repair version {version:04} checksum does not match"
            )));
        }
        if !matches!(
            row.status,
            MigrationStatus::DirtyApplying | MigrationStatus::DirtyFailed
        ) {
            return Err(fail(format!("repair version {version:04} is not dirty")));
        }
        match action {
            RepairAction::AcceptApplied => connection.execute(
                format!("UPDATE \"align_internal\".\"migrations_v1\" SET state=1,completed_statements={} WHERE version={} AND state IN (0,2) AND completed_statements=0", current.statement_count, version).as_bytes(),
                "PostgreSQL repair accept-applied",
            )?,
            RepairAction::ClearDirty => connection.execute(
                format!("DELETE FROM \"align_internal\".\"migrations_v1\" WHERE version={} AND state IN (0,2) AND completed_statements=0", version).as_bytes(),
                "PostgreSQL repair clear-dirty",
            )?,
        }
        let after = inspect_postgres_locked(connection, catalog)?;
        let expected = if action == RepairAction::AcceptApplied {
            MigrationStatus::Applied
        } else {
            MigrationStatus::Pending
        };
        if after
            .rows
            .iter()
            .find(|row| row.version == version)
            .map(|row| row.status)
            != Some(expected)
        {
            return Err(fail(
                "PostgreSQL repair did not produce the requested exact state",
            ));
        }
        Ok(after)
    })();
    match result {
        Ok(report) => {
            postgres_commit(
                connection,
                url,
                catalog,
                if action == RepairAction::AcceptApplied {
                    PostgresCommitExpectation::Applied(version)
                } else {
                    PostgresCommitExpectation::Cleared(version)
                },
                "PostgreSQL repair commit",
            )?;
            Ok(report)
        }
        Err(error) => {
            postgres_rollback(connection);
            Err(error)
        }
    }
}

pub fn run_postgres_migration(
    url: &str,
    operation: MigrationOperation<'_>,
    catalog: &ScreenedCatalog,
) -> Result<MigrationReport, MigrationError> {
    validate_postgres_migration_environment()?;
    validate_postgres_migration_url(url)?;
    let mut connection = PostgresConnection::open(url)?;
    connection.advisory_lock(operation.writes_database())?;
    match operation {
        MigrationOperation::Migrate => postgres_migrate(&mut connection, url, catalog),
        MigrationOperation::Status | MigrationOperation::Check => {
            postgres_inspect_phase(&connection, catalog, false, true)
        }
        MigrationOperation::Repair {
            version,
            action,
            expected_checksum,
        } => postgres_repair(
            &mut connection,
            url,
            catalog,
            version,
            action,
            expected_checksum,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db_prepare::{MigrationEntry, encode_migration_catalog};

    fn catalog(sql: &str) -> MigrationCatalog {
        encode_migration_catalog(vec![MigrationEntry {
            version: 1,
            filename: "0001_create.sql".to_string(),
            path: PathBuf::from("0001_create.sql"),
            bytes: sql.as_bytes().to_vec(),
        }])
        .unwrap()
    }

    fn temporary_path(name: &str) -> PathBuf {
        std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "align-db-migrate-{name}-{}-{}.sqlite",
                std::process::id(),
                std::thread::current().name().unwrap_or("test")
            ))
    }

    #[test]
    fn sqlite_required_migration_and_read_only_status_are_exact() {
        let path = temporary_path("required");
        let raw =
            catalog("CREATE TABLE users(id INTEGER PRIMARY KEY); INSERT INTO users VALUES (1);");
        let screened = screen_sqlite_catalog_native(&raw).unwrap();
        let report = run_sqlite_migration(&path, MigrationOperation::Migrate, &screened).unwrap();
        assert!(report.is_current());
        let status = run_sqlite_migration(&path, MigrationOperation::Status, &screened).unwrap();
        assert_eq!(status, report);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}.align-migrate.lock", path.display()));
    }

    #[test]
    fn sqlite_required_error_rolls_back_user_sql_and_history() {
        let path = temporary_path("rollback");
        let raw = catalog("CREATE TABLE partial(id INTEGER); SELECT no_such_function();");
        let screened = screen_sqlite_catalog_native(&raw).unwrap();
        assert!(run_sqlite_migration(&path, MigrationOperation::Migrate, &screened).is_err());
        let status = run_sqlite_migration(&path, MigrationOperation::Status, &screened).unwrap();
        assert_eq!(status.rows[0].status, MigrationStatus::Pending);
        let connection = SqliteConnection::open(&path, SQLITE_OPEN_READONLY).unwrap();
        assert!(
            connection
                .query("SELECT name FROM sqlite_schema WHERE name='partial'", 1)
                .unwrap()
                .is_empty()
        );
        drop(connection);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}.align-migrate.lock", path.display()));
    }

    #[test]
    fn sqlite_history_rejects_persistent_and_temporary_attached_behavior() {
        let path = temporary_path("history-objects");
        let raw = catalog("CREATE TABLE users(id INTEGER PRIMARY KEY);");
        let screened = screen_sqlite_catalog_native(&raw).unwrap();
        run_sqlite_migration(&path, MigrationOperation::Migrate, &screened).unwrap();

        let connection = SqliteConnection::open(&path, SQLITE_OPEN_READWRITE).unwrap();
        connection
            .execute(
                b"CREATE TEMP TRIGGER align_temp_history AFTER UPDATE ON main.__align_migrations_v1 BEGIN SELECT 1; END",
                "temporary trigger fixture",
            )
            .unwrap();
        assert!(
            sqlite_snapshot(&connection, &screened, true)
                .unwrap_err()
                .to_string()
                .contains("temporary")
        );
        drop(connection);

        let connection = SqliteConnection::open(&path, SQLITE_OPEN_READWRITE).unwrap();
        connection
            .execute(
                b"CREATE TABLE history_ref(version INTEGER REFERENCES __align_migrations_v1(version) ON DELETE CASCADE)",
                "inbound foreign key fixture",
            )
            .unwrap();
        drop(connection);
        assert!(
            run_sqlite_migration(&path, MigrationOperation::Status, &screened)
                .unwrap_err()
                .to_string()
                .contains("inbound foreign key")
        );
        let connection = SqliteConnection::open(&path, SQLITE_OPEN_READWRITE).unwrap();
        connection
            .execute(b"DROP TABLE history_ref", "inbound foreign key cleanup")
            .unwrap();
        drop(connection);

        let connection = SqliteConnection::open(&path, SQLITE_OPEN_READWRITE).unwrap();
        connection
            .execute(
                b"CREATE TRIGGER align_history AFTER UPDATE ON main.__align_migrations_v1 BEGIN SELECT 1; END",
                "persistent trigger fixture",
            )
            .unwrap();
        drop(connection);
        assert!(
            run_sqlite_migration(&path, MigrationOperation::Status, &screened)
                .unwrap_err()
                .to_string()
                .contains("attached objects")
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}.align-migrate.lock", path.display()));
    }

    #[test]
    fn sqlite_operation_lock_excludes_overlapping_writer_and_reader() {
        let path = temporary_path("lock");
        let exclusive = SqliteOperationLock::acquire(&path, true).unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || {
            let shared = SqliteOperationLock::acquire(&worker_path, false).unwrap();
            sent.send(()).unwrap();
            drop(shared);
        });
        assert!(
            received
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err()
        );
        drop(exclusive);
        received
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("reader acquires after writer release");
        worker.join().unwrap();
        let _ = std::fs::remove_file(format!("{}.align-migrate.lock", path.display()));
    }

    #[test]
    fn postgres_migration_validation_uses_migration_diagnostics() {
        assert!(
            validate_postgres_migration_url("not-a-url")
                .unwrap_err()
                .to_string()
                .contains("PostgreSQL migration requires")
        );
    }

    #[test]
    fn postgres_missing_history_sqlstates_cover_table_and_schema_absence() {
        for sqlstate in ["42P01", "3F000"] {
            assert!(postgres_history_is_missing(&PostgresCommandError {
                message: "missing history fixture".to_owned(),
                sqlstate: Some(sqlstate.to_owned()),
            }));
        }
        assert!(!postgres_history_is_missing(&PostgresCommandError {
            message: "unrelated failure fixture".to_owned(),
            sqlstate: Some("42501".to_owned()),
        }));
    }
}
