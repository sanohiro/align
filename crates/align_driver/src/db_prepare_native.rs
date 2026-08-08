//! Native SQLite/libpq describers for explicit `alignc db prepare` work.
//!
//! Libraries are loaded only when this explicit tool path runs. Keeping the function table owned by
//! the describer avoids making every `alignc` invocation depend on database client libraries.

use crate::db_prepare::{
    MetadataDescriber, MigrationCatalog, NativeColumnDescription, NativeParameterDescription,
    NativeStatementDescription, PreparationEnvironment, PrepareError,
};
use align_interface::{
    Driver, DriverEntry, Hash128, MetaNullability, StaticArtifact, StaticOptionValue,
};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::{Path, PathBuf};

fn fail(reason: impl Into<String>) -> PrepareError {
    PrepareError(reason.into())
}

#[cfg(unix)]
pub(crate) mod dynamic {
    use super::*;

    const RTLD_NOW: c_int = 2;

    #[cfg_attr(target_os = "linux", link(name = "dl"))]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    pub struct Library {
        handle: *mut c_void,
        name: String,
    }

    impl Library {
        pub fn open(candidates: &[&str]) -> Result<Self, PrepareError> {
            let mut failures = Vec::new();
            for candidate in candidates {
                let name = CString::new(*candidate)
                    .map_err(|_| fail("native library name contains NUL"))?;
                // SAFETY: `name` is a live NUL-terminated string and RTLD_NOW is a supported flag.
                let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW) };
                if !handle.is_null() {
                    return Ok(Self {
                        handle,
                        name: (*candidate).to_string(),
                    });
                }
                // SAFETY: `dlerror` returns either null or a process-owned NUL-terminated message.
                let message = unsafe {
                    let pointer = dlerror();
                    if pointer.is_null() {
                        "unknown loader error".to_string()
                    } else {
                        CStr::from_ptr(pointer).to_string_lossy().into_owned()
                    }
                };
                failures.push(format!("{candidate}: {message}"));
            }
            Err(fail(format!(
                "cannot load native database library ({})",
                failures.join("; ")
            )))
        }

        pub unsafe fn symbol<T: Copy>(&self, name: &'static [u8]) -> Result<T, PrepareError> {
            let name = CStr::from_bytes_with_nul(name)
                .map_err(|_| fail("native symbol name is not terminated"))?;
            // Clear a prior loader error before resolving this symbol.
            unsafe { dlerror() };
            // SAFETY: the library handle is live and `name` is NUL terminated.
            let pointer = unsafe { dlsym(self.handle, name.as_ptr()) };
            // SAFETY: this reads the thread-local loader error for the immediately preceding call.
            let error = unsafe { dlerror() };
            if pointer.is_null() || !error.is_null() {
                let detail = if error.is_null() {
                    "symbol resolved to null".to_string()
                } else {
                    // SAFETY: non-null `dlerror` text is NUL terminated and loader owned.
                    unsafe { CStr::from_ptr(error) }
                        .to_string_lossy()
                        .into_owned()
                };
                return Err(fail(format!(
                    "cannot resolve `{}` from {}: {detail}",
                    name.to_string_lossy(),
                    self.name
                )));
            }
            if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
                return Err(fail(
                    "native function pointer has an unsupported representation",
                ));
            }
            // SAFETY: the caller chooses the exact C function-pointer type for this named symbol;
            // the size equality above rejects non-pointer `T` representations.
            Ok(unsafe { std::mem::transmute_copy(&pointer) })
        }

        pub unsafe fn optional_symbol<T: Copy>(
            &self,
            name: &'static [u8],
        ) -> Result<Option<T>, PrepareError> {
            let name = CStr::from_bytes_with_nul(name)
                .map_err(|_| fail("native symbol name is not terminated"))?;
            // Clear a prior loader error before resolving this optional symbol.
            unsafe { dlerror() };
            // SAFETY: the library handle is live and `name` is NUL terminated.
            let pointer = unsafe { dlsym(self.handle, name.as_ptr()) };
            // SAFETY: this reads the thread-local loader error for the immediately preceding call.
            let error = unsafe { dlerror() };
            if pointer.is_null() || !error.is_null() {
                return Ok(None);
            }
            if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
                return Err(fail(
                    "native function pointer has an unsupported representation",
                ));
            }
            // SAFETY: the caller chooses the exact optional C function-pointer type for this named
            // symbol; the size equality above rejects non-pointer `T` representations.
            Ok(Some(unsafe { std::mem::transmute_copy(&pointer) }))
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                // SAFETY: this handle came from one successful `dlopen` and is closed once here.
                unsafe { dlclose(self.handle) };
                self.handle = std::ptr::null_mut();
            }
        }
    }
}

#[cfg(not(unix))]
pub(crate) mod dynamic {
    use super::*;

    pub struct Library;

    impl Library {
        pub fn open(_candidates: &[&str]) -> Result<Self, PrepareError> {
            Err(fail(
                "database preparation is not yet supported on this host",
            ))
        }

        pub unsafe fn symbol<T: Copy>(&self, _name: &'static [u8]) -> Result<T, PrepareError> {
            Err(fail(
                "database preparation is not yet supported on this host",
            ))
        }

        pub unsafe fn optional_symbol<T: Copy>(
            &self,
            _name: &'static [u8],
        ) -> Result<Option<T>, PrepareError> {
            Err(fail(
                "database preparation is not yet supported on this host",
            ))
        }
    }
}

type SqliteOpen =
    unsafe extern "C" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
type SqliteClose = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteErrmsg = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type SqliteVersion = unsafe extern "C" fn() -> *const c_char;
type SqliteVersionNumber = unsafe extern "C" fn() -> c_int;
type SqlitePrepare = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    c_int,
    *mut *mut c_void,
    *mut *const c_char,
) -> c_int;
type SqliteFinalize = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteCount = unsafe extern "C" fn(*mut c_void) -> c_int;
type SqliteIndexedText = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;
type SqliteExec = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
    *mut c_void,
    *mut *mut c_char,
) -> c_int;
type SqliteFree = unsafe extern "C" fn(*mut c_void);

struct SqliteApi {
    _library: dynamic::Library,
    open_v2: SqliteOpen,
    close_v2: SqliteClose,
    errmsg: SqliteErrmsg,
    libversion: SqliteVersion,
    libversion_number: SqliteVersionNumber,
    prepare_v2: SqlitePrepare,
    finalize: SqliteFinalize,
    bind_parameter_count: SqliteCount,
    bind_parameter_name: SqliteIndexedText,
    column_count: SqliteCount,
    column_name: SqliteIndexedText,
    column_decltype: SqliteIndexedText,
    column_database_name: Option<SqliteIndexedText>,
    column_table_name: Option<SqliteIndexedText>,
    column_origin_name: Option<SqliteIndexedText>,
    exec: SqliteExec,
    free: SqliteFree,
}

impl SqliteApi {
    fn load() -> Result<Self, PrepareError> {
        #[cfg(target_os = "macos")]
        let candidates = [
            "/opt/homebrew/opt/sqlite/lib/libsqlite3.dylib",
            "/usr/local/opt/sqlite/lib/libsqlite3.dylib",
            "libsqlite3.dylib",
            "/usr/lib/libsqlite3.dylib",
        ];
        #[cfg(not(target_os = "macos"))]
        let candidates = ["libsqlite3.so.0", "libsqlite3.so", "libsqlite3.dylib", ""];
        let library = dynamic::Library::open(
            &candidates
                .iter()
                .copied()
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>(),
        )?;
        // SAFETY: every symbol is assigned its documented sqlite3 C signature and the library is
        // retained in the same table for longer than any call.
        unsafe {
            Ok(Self {
                open_v2: library.symbol(b"sqlite3_open_v2\0")?,
                close_v2: library.symbol(b"sqlite3_close_v2\0")?,
                errmsg: library.symbol(b"sqlite3_errmsg\0")?,
                libversion: library.symbol(b"sqlite3_libversion\0")?,
                libversion_number: library.symbol(b"sqlite3_libversion_number\0")?,
                prepare_v2: library.symbol(b"sqlite3_prepare_v2\0")?,
                finalize: library.symbol(b"sqlite3_finalize\0")?,
                bind_parameter_count: library.symbol(b"sqlite3_bind_parameter_count\0")?,
                bind_parameter_name: library.symbol(b"sqlite3_bind_parameter_name\0")?,
                column_count: library.symbol(b"sqlite3_column_count\0")?,
                column_name: library.symbol(b"sqlite3_column_name\0")?,
                column_decltype: library.symbol(b"sqlite3_column_decltype\0")?,
                column_database_name: library.optional_symbol(b"sqlite3_column_database_name\0")?,
                column_table_name: library.optional_symbol(b"sqlite3_column_table_name\0")?,
                column_origin_name: library.optional_symbol(b"sqlite3_column_origin_name\0")?,
                exec: library.symbol(b"sqlite3_exec\0")?,
                free: library.symbol(b"sqlite3_free\0")?,
                _library: library,
            })
        }
    }
}

enum SqliteSource {
    Database(PathBuf),
    Memory(Option<MigrationCatalog>),
}

pub struct SqliteDescriber {
    api: Option<SqliteApi>,
    database: *mut c_void,
    source: SqliteSource,
    schema_fingerprint: Hash128,
}

impl SqliteDescriber {
    pub fn database(path: &Path, schema_fingerprint: Hash128) -> Self {
        Self {
            api: None,
            database: std::ptr::null_mut(),
            source: SqliteSource::Database(path.to_path_buf()),
            schema_fingerprint,
        }
    }

    pub fn memory(schema_fingerprint: Hash128) -> Self {
        Self {
            api: None,
            database: std::ptr::null_mut(),
            source: SqliteSource::Memory(None),
            schema_fingerprint,
        }
    }

    pub fn memory_with_migrations(catalog: MigrationCatalog) -> Self {
        Self {
            api: None,
            database: std::ptr::null_mut(),
            schema_fingerprint: crate::db_prepare::sqlite_memory_schema_fingerprint(Some(
                catalog.fingerprint,
            )),
            source: SqliteSource::Memory(Some(catalog)),
        }
    }

    fn open(&mut self) -> Result<(), PrepareError> {
        if !self.database.is_null() {
            return Ok(());
        }
        let api = SqliteApi::load()?;
        let (name, flags) = match &self.source {
            SqliteSource::Database(path) => (
                CString::new(path.as_os_str().as_encoded_bytes())
                    .map_err(|_| fail("SQLite database path contains U+0000"))?,
                1,
            ),
            SqliteSource::Memory(_) => (
                CString::new(":memory:").map_err(|_| fail("invalid SQLite memory name"))?,
                2 | 4,
            ),
        };
        let mut database = std::ptr::null_mut();
        // SAFETY: all pointers are live for the call and `database` is valid output storage.
        let status =
            unsafe { (api.open_v2)(name.as_ptr(), &mut database, flags, std::ptr::null()) };
        if status != 0 || database.is_null() {
            let message = if database.is_null() {
                format!("SQLite open failed with status {status}")
            } else {
                sqlite_text(unsafe { (api.errmsg)(database) }, "SQLite open error")
                    .unwrap_or_else(|_| format!("SQLite open failed with status {status}"))
            };
            if !database.is_null() {
                // SAFETY: partial `sqlite3_open_v2` handles must be closed once.
                unsafe { (api.close_v2)(database) };
            }
            return Err(fail(message));
        }
        self.database = database;
        self.api = Some(api);
        if let SqliteSource::Memory(Some(catalog)) = &self.source {
            let scripts = catalog
                .entries
                .iter()
                .map(|entry| entry.bytes.clone())
                .collect::<Vec<_>>();
            if let Err(error) = self.apply_migrations(&scripts) {
                self.close();
                return Err(error);
            }
        }
        if let Err(error) = self.execute_script(
            b"BEGIN; SELECT rootpage FROM sqlite_schema LIMIT 1;",
            "SQLite schema snapshot",
        ) {
            self.close();
            return Err(error);
        }
        Ok(())
    }

    fn execute_script(&self, bytes: &[u8], context: &str) -> Result<(), PrepareError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("SQLite function table is absent"))?;
        let sql = CString::new(bytes).map_err(|_| fail(format!("{context} contains U+0000")))?;
        let mut error_message = std::ptr::null_mut();
        // SAFETY: the connection and SQL are live for this synchronous call; no callback is used.
        let status = unsafe {
            (api.exec)(
                self.database,
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                &mut error_message,
            )
        };
        if status == 0 {
            return Ok(());
        }
        let detail = if error_message.is_null() {
            format!("{context} failed with status {status}")
        } else {
            let text = sqlite_text(error_message, "SQLite migration error")
                .unwrap_or_else(|_| format!("{context} failed with status {status}"));
            // SAFETY: sqlite3_exec allocated this error string for the caller.
            unsafe { (api.free)(error_message.cast()) };
            text
        };
        Err(fail(detail))
    }

    fn apply_migrations(&self, scripts: &[Vec<u8>]) -> Result<(), PrepareError> {
        self.execute_script(b"BEGIN IMMEDIATE", "SQLite migration transaction")?;
        for (index, script) in scripts.iter().enumerate() {
            if let Err(error) =
                self.execute_script(script, &format!("SQLite migration {:04}", index + 1))
            {
                let _ = self.execute_script(b"ROLLBACK", "SQLite migration rollback");
                return Err(error);
            }
        }
        if let Err(error) = self.execute_script(b"COMMIT", "SQLite migration commit") {
            let _ = self.execute_script(b"ROLLBACK", "SQLite migration rollback");
            return Err(error);
        }
        Ok(())
    }

    fn close(&mut self) {
        if !self.database.is_null() {
            if let Some(api) = &self.api {
                // SAFETY: this describer owns one open connection and closes it at most once.
                unsafe { (api.close_v2)(self.database) };
            }
            self.database = std::ptr::null_mut();
        }
    }
}

fn sqlite_text(pointer: *const c_char, what: &str) -> Result<String, PrepareError> {
    if pointer.is_null() {
        return Err(fail(format!("{what} is null")));
    }
    // SAFETY: SQLite documents these returned values as NUL-terminated strings valid through the
    // surrounding statement/connection call, and the caller copies them immediately.
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map(str::to_string)
        .map_err(|_| fail(format!("{what} is not UTF-8")))
}

fn sqlite_optional_text(
    pointer: *const c_char,
    what: &str,
) -> Result<Option<String>, PrepareError> {
    if pointer.is_null() {
        Ok(None)
    } else {
        sqlite_text(pointer, what).map(Some)
    }
}

fn sqlite_optional_origin(
    function: Option<SqliteIndexedText>,
    statement: *mut c_void,
    index: c_int,
    what: &str,
) -> Result<Option<String>, PrepareError> {
    let Some(function) = function else {
        return Ok(None);
    };
    // SAFETY: the optional symbol has SQLite's indexed column-text signature, and the caller keeps
    // the prepared statement live while the returned text is copied.
    sqlite_optional_text(unsafe { function(statement, index) }, what)
}

fn sqlite_tail_empty(bytes: &[u8]) -> bool {
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
        } else if bytes.get(index..index + 2) == Some(b"--") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if bytes.get(index..index + 2) == Some(b"/*") {
            let Some(end) = bytes[index + 2..]
                .windows(2)
                .position(|window| window == b"*/")
            else {
                return false;
            };
            index += end + 4;
        } else {
            return false;
        }
    }
    true
}

fn static_options(artifact: &StaticArtifact) -> &[align_interface::StaticOption] {
    match artifact {
        StaticArtifact::Query(query) => &query.static_options,
        StaticArtifact::Command(command) => &command.static_options,
    }
}

fn require_sqlite_version(api: &SqliteApi, artifact: &StaticArtifact) -> Result<(), PrepareError> {
    // SAFETY: the function table retains the loaded SQLite library for this call.
    let encoded = unsafe { (api.libversion_number)() };
    if encoded <= 0 {
        return Err(fail("SQLite library version number is unavailable"));
    }
    let encoded =
        u32::try_from(encoded).map_err(|_| fail("SQLite library version number is invalid"))?;
    let actual = (
        encoded / 1_000_000,
        (encoded / 1_000) % 1_000,
        encoded % 1_000,
    );
    for option in static_options(artifact) {
        if let StaticOptionValue::SQLiteRequireVersionAtLeast {
            major,
            minor,
            patch,
        } = option.value
        {
            let required = (major, minor, patch);
            if actual < required {
                return Err(fail(format!(
                    "SQLite {}.{}.{} is older than required version {major}.{minor}.{patch}",
                    actual.0, actual.1, actual.2
                )));
            }
        }
    }
    Ok(())
}

impl MetadataDescriber for SqliteDescriber {
    fn driver(&self) -> Driver {
        Driver::SQLite
    }

    fn environment(&mut self) -> Result<PreparationEnvironment, PrepareError> {
        self.open()?;
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("SQLite function table is absent"))?;
        // SAFETY: the loaded function table and connection stay live in `self`.
        let version = sqlite_text(unsafe { (api.libversion)() }, "SQLite library version")?;
        Ok(PreparationEnvironment {
            engine_version: version.clone(),
            driver_version: version,
            schema_fingerprint: self.schema_fingerprint,
            search_path: Vec::new(),
            extensions: Vec::new(),
        })
    }

    fn describe(
        &mut self,
        artifact: &StaticArtifact,
        entry: &DriverEntry,
    ) -> Result<NativeStatementDescription, PrepareError> {
        self.open()?;
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("SQLite function table is absent"))?;
        require_sqlite_version(api, artifact)?;
        let sql = CString::new(entry.wire_sql.as_slice())
            .map_err(|_| fail("SQLite SQL contains U+0000"))?;
        let sql_len = c_int::try_from(entry.wire_sql.len())
            .map_err(|_| fail("SQLite SQL exceeds i32 length"))?;
        let mut statement = std::ptr::null_mut();
        let mut tail = std::ptr::null();
        // SAFETY: SQL and output pointers are valid and the connection is live.
        let status = unsafe {
            (api.prepare_v2)(
                self.database,
                sql.as_ptr(),
                sql_len,
                &mut statement,
                &mut tail,
            )
        };
        if status != 0 || statement.is_null() {
            let message = sqlite_text(
                unsafe { (api.errmsg)(self.database) },
                "SQLite prepare error",
            )
            .unwrap_or_else(|_| format!("SQLite prepare failed with status {status}"));
            if !statement.is_null() {
                // SAFETY: partial prepared statement is finalized once.
                unsafe { (api.finalize)(statement) };
            }
            return Err(fail(message));
        }

        let result = (|| {
            let start = sql.as_ptr() as usize;
            let tail_address = tail as usize;
            let offset = tail_address
                .checked_sub(start)
                .ok_or_else(|| fail("SQLite returned a tail before SQL"))?;
            if offset > entry.wire_sql.len() || !sqlite_tail_empty(&entry.wire_sql[offset..]) {
                return Err(fail("SQLite prepared SQL contains more than one statement"));
            }
            // SAFETY: all count/name/column calls use the live prepared statement.
            let parameter_count = unsafe { (api.bind_parameter_count)(statement) };
            if parameter_count < 0 {
                return Err(fail("SQLite returned a negative parameter count"));
            }
            let parameter_capacity = usize::try_from(parameter_count)
                .map_err(|_| fail("SQLite parameter count exceeds usize"))?;
            let mut parameters = Vec::with_capacity(parameter_capacity);
            for index in 1..=parameter_count {
                let raw_name = unsafe { (api.bind_parameter_name)(statement, index) };
                let name = sqlite_optional_text(raw_name, "SQLite parameter name")?.map(|name| {
                    name.strip_prefix([':', '@', '$'])
                        .unwrap_or(&name)
                        .to_string()
                });
                parameters.push(NativeParameterDescription {
                    ordinal: u32::try_from(index)
                        .map_err(|_| fail("SQLite parameter ordinal overflow"))?,
                    source_name: name,
                    native_type: None,
                    native_type_id: None,
                });
            }
            let column_count = unsafe { (api.column_count)(statement) };
            if column_count < 0 {
                return Err(fail("SQLite returned a negative column count"));
            }
            let column_capacity = usize::try_from(column_count)
                .map_err(|_| fail("SQLite column count exceeds usize"))?;
            let mut columns = Vec::with_capacity(column_capacity);
            for index in 0..column_count {
                let source_alias = sqlite_text(
                    unsafe { (api.column_name)(statement, index) },
                    "SQLite result column name",
                )?;
                columns.push(NativeColumnDescription {
                    ordinal: u32::try_from(index)
                        .map_err(|_| fail("SQLite column ordinal overflow"))?,
                    source_alias,
                    native_type: sqlite_optional_text(
                        unsafe { (api.column_decltype)(statement, index) },
                        "SQLite declared column type",
                    )?,
                    native_type_id: None,
                    origin_schema: sqlite_optional_origin(
                        api.column_database_name,
                        statement,
                        index,
                        "SQLite origin schema",
                    )?,
                    origin_table: sqlite_optional_origin(
                        api.column_table_name,
                        statement,
                        index,
                        "SQLite origin table",
                    )?,
                    origin_column: sqlite_optional_origin(
                        api.column_origin_name,
                        statement,
                        index,
                        "SQLite origin column",
                    )?,
                    nullable: MetaNullability::Unknown,
                });
            }
            if matches!(artifact, StaticArtifact::Command(_)) && !columns.is_empty() {
                return Err(fail(
                    "SQLite statement result kind disagrees with the static descriptor",
                ));
            }
            Ok(NativeStatementDescription {
                parameters,
                columns,
            })
        })();

        // SAFETY: the statement was produced above and is finalized exactly once here.
        let finalize = unsafe { (api.finalize)(statement) };
        if let Err(error) = result {
            return Err(error);
        }
        if finalize != 0 {
            return Err(fail(format!(
                "SQLite finalize failed with status {finalize}"
            )));
        }
        result
    }
}

impl Drop for SqliteDescriber {
    fn drop(&mut self) {
        self.close();
    }
}

type PqConnectParams =
    unsafe extern "C" fn(*const *const c_char, *const *const c_char, c_int) -> *mut c_void;
type PqFinish = unsafe extern "C" fn(*mut c_void);
type PqStatus = unsafe extern "C" fn(*const c_void) -> c_int;
type PqClientEncoding = unsafe extern "C" fn(*const c_void) -> c_int;
type PqServerVersion = unsafe extern "C" fn(*const c_void) -> c_int;
type PqLibVersion = unsafe extern "C" fn() -> c_int;
type PqExec = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type PqPrepare = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    c_int,
    *const u32,
) -> *mut c_void;
type PqDescribePrepared = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type PqClear = unsafe extern "C" fn(*mut c_void);
type PqResultStatus = unsafe extern "C" fn(*const c_void) -> c_int;
type PqResultErrorMessage = unsafe extern "C" fn(*const c_void) -> *const c_char;
type PqCount = unsafe extern "C" fn(*const c_void) -> c_int;
type PqGetIsNull = unsafe extern "C" fn(*const c_void, c_int, c_int) -> c_int;
type PqGetValue = unsafe extern "C" fn(*const c_void, c_int, c_int) -> *mut c_char;
type PqIndexedName = unsafe extern "C" fn(*const c_void, c_int) -> *const c_char;
type PqIndexedOid = unsafe extern "C" fn(*const c_void, c_int) -> u32;
type PqIndexedInt = unsafe extern "C" fn(*const c_void, c_int) -> c_int;

struct PostgresApi {
    _library: dynamic::Library,
    connectdb_params: PqConnectParams,
    finish: PqFinish,
    status: PqStatus,
    client_encoding: PqClientEncoding,
    server_version: PqServerVersion,
    lib_version: PqLibVersion,
    exec: PqExec,
    prepare: PqPrepare,
    describe_prepared: PqDescribePrepared,
    clear: PqClear,
    result_status: PqResultStatus,
    result_error_message: PqResultErrorMessage,
    ntuples: PqCount,
    nfields: PqCount,
    nparams: PqCount,
    getisnull: PqGetIsNull,
    getvalue: PqGetValue,
    fname: PqIndexedName,
    ftype: PqIndexedOid,
    ftable: PqIndexedOid,
    ftablecol: PqIndexedInt,
    paramtype: PqIndexedOid,
}

impl PostgresApi {
    fn load() -> Result<Self, PrepareError> {
        #[cfg(target_os = "macos")]
        let candidates = [
            "/opt/homebrew/opt/libpq/lib/libpq.5.dylib",
            "/usr/local/opt/libpq/lib/libpq.5.dylib",
            "libpq.5.dylib",
            "libpq.dylib",
        ];
        #[cfg(not(target_os = "macos"))]
        let candidates = ["libpq.so.5", "libpq.so", "libpq.5.dylib", ""];
        let library = dynamic::Library::open(
            &candidates
                .iter()
                .copied()
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>(),
        )?;
        // SAFETY: every entry below uses libpq's exact C function-pointer signature and the table
        // owns the dynamic library for the complete lifetime of every call.
        unsafe {
            Ok(Self {
                connectdb_params: library.symbol(b"PQconnectdbParams\0")?,
                finish: library.symbol(b"PQfinish\0")?,
                status: library.symbol(b"PQstatus\0")?,
                client_encoding: library.symbol(b"PQclientEncoding\0")?,
                server_version: library.symbol(b"PQserverVersion\0")?,
                lib_version: library.symbol(b"PQlibVersion\0")?,
                exec: library.symbol(b"PQexec\0")?,
                prepare: library.symbol(b"PQprepare\0")?,
                describe_prepared: library.symbol(b"PQdescribePrepared\0")?,
                clear: library.symbol(b"PQclear\0")?,
                result_status: library.symbol(b"PQresultStatus\0")?,
                result_error_message: library.symbol(b"PQresultErrorMessage\0")?,
                ntuples: library.symbol(b"PQntuples\0")?,
                nfields: library.symbol(b"PQnfields\0")?,
                nparams: library.symbol(b"PQnparams\0")?,
                getisnull: library.symbol(b"PQgetisnull\0")?,
                getvalue: library.symbol(b"PQgetvalue\0")?,
                fname: library.symbol(b"PQfname\0")?,
                ftype: library.symbol(b"PQftype\0")?,
                ftable: library.symbol(b"PQftable\0")?,
                ftablecol: library.symbol(b"PQftablecol\0")?,
                paramtype: library.symbol(b"PQparamtype\0")?,
                _library: library,
            })
        }
    }
}

pub struct PostgresDescriber {
    api: Option<PostgresApi>,
    connection: *mut c_void,
    url: String,
    schema_id: String,
    type_names: std::collections::HashMap<u32, String>,
}

type PostgresOrigin = (Option<String>, Option<String>, Option<String>);

pub(crate) fn validate_complete_postgres_url(url: &str) -> Result<(), PrepareError> {
    let rest = url
        .strip_prefix("postgresql://")
        .or_else(|| url.strip_prefix("postgres://"))
        .ok_or_else(|| fail("PostgreSQL preparation requires a complete postgresql:// URL"))?;
    if rest.contains('#') {
        return Err(fail(
            "PostgreSQL preparation URL must not contain a fragment",
        ));
    }
    let (authority, path_and_query) = rest
        .split_once('/')
        .ok_or_else(|| fail("PostgreSQL preparation URL must include an explicit database"))?;
    let (userinfo, host_port) = authority.rsplit_once('@').ok_or_else(|| {
        fail("PostgreSQL preparation URL must include an explicit user and password")
    })?;
    let (user, password) = userinfo
        .split_once(':')
        .ok_or_else(|| fail("PostgreSQL preparation URL must include an explicit password"))?;
    if user.is_empty() || password.is_empty() {
        return Err(fail(
            "PostgreSQL preparation URL must include a non-empty user and password",
        ));
    }
    let (host, port) = if let Some(ipv6) = host_port.strip_prefix('[') {
        let (host, port) = ipv6
            .split_once("]:")
            .ok_or_else(|| fail("PostgreSQL preparation URL must include an explicit port"))?;
        (host, port)
    } else {
        host_port
            .rsplit_once(':')
            .ok_or_else(|| fail("PostgreSQL preparation URL must include an explicit port"))?
    };
    if host.is_empty() || host.contains(',') || host.contains('%') {
        return Err(fail(
            "PostgreSQL preparation URL must select exactly one explicit host",
        ));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| fail("PostgreSQL preparation URL has an invalid explicit port"))?;
    if port == 0 {
        return Err(fail("PostgreSQL preparation URL port must be non-zero"));
    }
    let (database, query) = path_and_query
        .split_once('?')
        .map_or((path_and_query, None), |(database, query)| {
            (database, Some(query))
        });
    if database.is_empty() || database.contains('/') {
        return Err(fail(
            "PostgreSQL preparation URL must select exactly one explicit database",
        ));
    }
    if let Some(query) = query {
        for parameter in query.split('&') {
            let key = parameter.split_once('=').map_or(parameter, |(key, _)| key);
            if key.contains('%')
                || matches!(
                    key,
                    "host"
                        | "hostaddr"
                        | "port"
                        | "dbname"
                        | "database"
                        | "user"
                        | "password"
                        | "service"
                        | "servicefile"
                        | "target_session_attrs"
                        | "load_balance_hosts"
                        | "client_encoding"
                        | "options"
                )
            {
                return Err(fail(
                    "PostgreSQL preparation URL contains a forbidden target or startup override",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn reject_ambient_postgres_environment() -> Result<(), PrepareError> {
    if std::env::vars_os().any(|(name, _)| {
        name.to_str()
            .is_some_and(|name| name.as_bytes().starts_with(b"PG"))
    }) {
        return Err(fail(
            "PostgreSQL preparation rejects ambient PG* environment variables",
        ));
    }
    Ok(())
}

fn postgres_parameter_oids(
    artifact: &StaticArtifact,
    entry: &DriverEntry,
) -> Result<Vec<u32>, PrepareError> {
    let parameter_count = entry.bindings.len();
    let mut oids = vec![0; parameter_count];
    let mut ordinals = vec![false; parameter_count];
    for binding in &entry.bindings {
        let index = binding
            .protocol_ordinal
            .checked_sub(1)
            .and_then(|ordinal| usize::try_from(ordinal).ok())
            .filter(|index| *index < parameter_count)
            .ok_or_else(|| fail("PostgreSQL parameter ordinal is not dense"))?;
        if std::mem::replace(&mut ordinals[index], true) {
            return Err(fail("PostgreSQL parameter ordinal is duplicated"));
        }
    }
    for option in static_options(artifact) {
        let StaticOptionValue::PostgreSQLParameterType {
            parameter_name,
            canonical_type_name,
        } = &option.value
        else {
            continue;
        };
        let oid = match canonical_type_name.as_str() {
            "int8" => 20,
            _ => {
                return Err(fail(format!(
                    "unsupported PostgreSQL parameter type `{canonical_type_name}`"
                )));
            }
        };
        let binding = entry
            .bindings
            .iter()
            .find(|binding| binding.source_name == *parameter_name)
            .ok_or_else(|| {
                fail(format!(
                    "PostgreSQL parameter type names unknown Params field `{parameter_name}`"
                ))
            })?;
        let index = binding
            .protocol_ordinal
            .checked_sub(1)
            .and_then(|ordinal| usize::try_from(ordinal).ok())
            .filter(|index| *index < oids.len())
            .ok_or_else(|| fail("PostgreSQL parameter ordinal is not dense"))?;
        if oids[index] != 0 {
            return Err(fail(format!(
                "duplicate PostgreSQL parameter type for `{parameter_name}`"
            )));
        }
        oids[index] = oid;
    }
    Ok(oids)
}

impl PostgresDescriber {
    pub fn new(url: String, schema_id: String) -> Self {
        Self {
            api: None,
            connection: std::ptr::null_mut(),
            url,
            schema_id,
            type_names: std::collections::HashMap::new(),
        }
    }

    fn open(&mut self) -> Result<(), PrepareError> {
        if !self.connection.is_null() {
            return Ok(());
        }
        validate_complete_postgres_url(&self.url)?;
        reject_ambient_postgres_environment()?;
        let api = PostgresApi::load()?;
        let url = CString::new(self.url.as_bytes())
            .map_err(|_| fail("PostgreSQL URL contains U+0000"))?;
        let dbname = CString::new("dbname").map_err(|_| fail("invalid libpq keyword"))?;
        let client_encoding =
            CString::new("client_encoding").map_err(|_| fail("invalid libpq keyword"))?;
        let options = CString::new("options").map_err(|_| fail("invalid libpq keyword"))?;
        let utf8 = CString::new("UTF8").map_err(|_| fail("invalid libpq value"))?;
        // One ASCII space is a non-empty libpq value (so PGOPTIONS is not consulted) and an empty
        // startup-option sequence after server tokenization.
        let no_startup_options = CString::new(" ").map_err(|_| fail("invalid libpq value"))?;
        let keywords = [
            dbname.as_ptr(),
            client_encoding.as_ptr(),
            options.as_ptr(),
            std::ptr::null(),
        ];
        let values = [
            url.as_ptr(),
            utf8.as_ptr(),
            no_startup_options.as_ptr(),
            std::ptr::null(),
        ];
        // SAFETY: both pointer arrays and every referenced C string stay live for this synchronous
        // call. Expansion parses the first `dbname` URL; the later package-owned UTF-8 value wins.
        let connection = unsafe { (api.connectdb_params)(keywords.as_ptr(), values.as_ptr(), 1) };
        if connection.is_null() {
            return Err(fail("libpq returned a null PostgreSQL connection"));
        }
        // SAFETY: the connection is live until the failure cleanup or ownership transfer below.
        if unsafe { (api.status)(connection) } != 0 {
            unsafe { (api.finish)(connection) };
            // Native connection diagnostics may echo arbitrary connection-string fields. Keep
            // every URL credential out of tool output by returning only package-owned text.
            return Err(fail("PostgreSQL connection failed"));
        }
        // Align's package/runtime contract fixes the client encoding to UTF-8 (libpq tag 6).
        if unsafe { (api.client_encoding)(connection) } != 6 {
            unsafe { (api.finish)(connection) };
            return Err(fail("PostgreSQL client encoding is not UTF-8"));
        }
        self.connection = connection;
        self.api = Some(api);
        if let Err(error) = self.execute_command(
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            "PostgreSQL schema snapshot",
        ) {
            self.close();
            return Err(error);
        }
        Ok(())
    }

    fn close(&mut self) {
        if !self.connection.is_null() {
            if let Some(api) = &self.api {
                // SAFETY: this describer owns one PGconn and releases it at most once.
                unsafe { (api.finish)(self.connection) };
            }
            self.connection = std::ptr::null_mut();
        }
    }

    fn query_rows(
        &self,
        sql: &str,
        expected_fields: usize,
    ) -> Result<Vec<Vec<Option<String>>>, PrepareError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("PostgreSQL function table is absent"))?;
        let sql = CString::new(sql).map_err(|_| fail("PostgreSQL tool query contains U+0000"))?;
        // SAFETY: the connection and SQL are live for this synchronous libpq call.
        let result = unsafe { (api.exec)(self.connection, sql.as_ptr()) };
        if result.is_null() {
            return Err(fail("libpq returned a null PostgreSQL result"));
        }
        let decoded = (|| {
            let status = unsafe { (api.result_status)(result) };
            if status != 2 {
                return Err(fail(
                    pq_text(
                        unsafe { (api.result_error_message)(result) },
                        "PostgreSQL query error",
                    )
                    .unwrap_or_else(|_| format!("PostgreSQL query failed with status {status}")),
                ));
            }
            let rows = unsafe { (api.ntuples)(result) };
            let fields = unsafe { (api.nfields)(result) };
            if rows < 0 || fields < 0 || usize::try_from(fields).ok() != Some(expected_fields) {
                return Err(fail("PostgreSQL tool query returned an invalid shape"));
            }
            let row_count =
                usize::try_from(rows).map_err(|_| fail("PostgreSQL row count exceeds usize"))?;
            let mut output = Vec::with_capacity(row_count);
            for row in 0..rows {
                let mut values = Vec::with_capacity(expected_fields);
                for field in 0..fields {
                    if unsafe { (api.getisnull)(result, row, field) } != 0 {
                        values.push(None);
                    } else {
                        values.push(Some(pq_text(
                            unsafe { (api.getvalue)(result, row, field) },
                            "PostgreSQL query value",
                        )?));
                    }
                }
                output.push(values);
            }
            Ok(output)
        })();
        // SAFETY: every non-null PGresult is cleared exactly once after copying its fields.
        unsafe { (api.clear)(result) };
        decoded
    }

    fn execute_command(&self, sql: &str, context: &str) -> Result<(), PrepareError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("PostgreSQL function table is absent"))?;
        let sql = CString::new(sql).map_err(|_| fail(format!("{context} contains U+0000")))?;
        // SAFETY: the connection and command text are live for this synchronous call.
        let result = unsafe { (api.exec)(self.connection, sql.as_ptr()) };
        if result.is_null() {
            return Err(fail(format!("libpq returned a null {context} result")));
        }
        let status = unsafe { (api.result_status)(result) };
        let error = if status == 1 {
            None
        } else {
            Some(fail(
                pq_text(
                    unsafe { (api.result_error_message)(result) },
                    &format!("{context} error"),
                )
                .unwrap_or_else(|_| format!("{context} failed with status {status}")),
            ))
        };
        // SAFETY: this non-null PGresult is cleared exactly once after copying its error text.
        unsafe { (api.clear)(result) };
        match error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn type_name(&mut self, oid: u32) -> Result<String, PrepareError> {
        if let Some(name) = self.type_names.get(&oid) {
            return Ok(name.clone());
        }
        let rows = self.query_rows(&format!("SELECT pg_catalog.format_type({oid},NULL)"), 1)?;
        let name = rows
            .first()
            .and_then(|row| row.first())
            .and_then(Option::as_ref)
            .ok_or_else(|| fail(format!("PostgreSQL type OID {oid} has no canonical name")))?
            .clone();
        self.type_names.insert(oid, name.clone());
        Ok(name)
    }

    fn origin(&self, table_oid: u32, attribute: c_int) -> Result<PostgresOrigin, PrepareError> {
        if table_oid == 0 || attribute <= 0 {
            return Ok((None, None, None));
        }
        let rows = self.query_rows(
            &format!(
                "SELECT n.nspname,c.relname,a.attname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid WHERE c.oid={table_oid} AND a.attnum={attribute}"
            ),
            3,
        )?;
        let Some(row) = rows.first() else {
            return Ok((None, None, None));
        };
        let [schema, table, column] = row.as_slice() else {
            return Err(fail("PostgreSQL origin query returned an invalid shape"));
        };
        Ok((schema.clone(), table.clone(), column.clone()))
    }

    fn deallocate(&self, name: &str) -> Result<(), PrepareError> {
        self.execute_command(&format!("DEALLOCATE \"{name}\""), "PostgreSQL DEALLOCATE")
    }
}

fn pq_text(pointer: *const c_char, what: &str) -> Result<String, PrepareError> {
    if pointer.is_null() {
        return Err(fail(format!("{what} is null")));
    }
    // SAFETY: libpq returns NUL-terminated storage owned by its connection/result; callers copy it
    // before clearing that owner.
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map(str::to_string)
        .map_err(|_| fail(format!("{what} is not UTF-8")))
}

fn postgres_version(value: c_int) -> Result<String, PrepareError> {
    if value <= 0 {
        return Err(fail("PostgreSQL version is unavailable"));
    }
    let major = value / 10000;
    if value >= 100000 {
        return Ok(format!("{major}.{}", value % 10000));
    }
    let minor = (value % 10000) / 100;
    let patch = value % 100;
    Ok(format!("{major}.{minor}.{patch}"))
}

impl MetadataDescriber for PostgresDescriber {
    fn driver(&self) -> Driver {
        Driver::PostgreSQL
    }

    fn environment(&mut self) -> Result<PreparationEnvironment, PrepareError> {
        self.open()?;
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("PostgreSQL function table is absent"))?;
        let engine_version = postgres_version(unsafe { (api.server_version)(self.connection) })?;
        let driver_version = postgres_version(unsafe { (api.lib_version)() })?;
        let search_path = self
            .query_rows(
                "SELECT pg_catalog.unnest(pg_catalog.current_schemas(true))",
                1,
            )?
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .next()
                    .flatten()
                    .ok_or_else(|| fail("PostgreSQL search path contains NULL"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut extensions = self.query_rows(
            "SELECT n.nspname,e.extname,e.extversion FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace ORDER BY n.nspname,e.extname,e.extversion",
            3,
        )?.into_iter().map(|row| {
            let [schema, name, version] = row.as_slice() else {
                return Err(fail("PostgreSQL extension query returned an invalid shape"));
            };
            Ok(crate::db_prepare::PreparationExtension {
                schema: schema.clone().ok_or_else(|| fail("PostgreSQL extension schema is NULL"))?,
                name: name.clone().ok_or_else(|| fail("PostgreSQL extension name is NULL"))?,
                version: version.clone(),
            })
        }).collect::<Result<Vec<_>, PrepareError>>()?;
        extensions.sort();
        let schema_fingerprint = crate::db_prepare::postgres_schema_fingerprint(
            &self.schema_id,
            &search_path,
            &extensions,
        )?;
        Ok(PreparationEnvironment {
            engine_version,
            driver_version,
            schema_fingerprint,
            search_path,
            extensions,
        })
    }

    fn describe(
        &mut self,
        artifact: &StaticArtifact,
        entry: &DriverEntry,
    ) -> Result<NativeStatementDescription, PrepareError> {
        self.open()?;
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| fail("PostgreSQL function table is absent"))?;
        let descriptor_id = match artifact {
            StaticArtifact::Query(query) => &query.query_id,
            StaticArtifact::Command(command) => &command.command_id,
        };
        let name = format!(
            "align_q3_{}",
            Hash128::of(descriptor_id.as_bytes()).to_hex()
        );
        let name_c = CString::new(name.as_bytes())
            .map_err(|_| fail("PostgreSQL prepared name contains U+0000"))?;
        let sql = CString::new(entry.wire_sql.as_slice())
            .map_err(|_| fail("PostgreSQL SQL contains U+0000"))?;
        let parameter_oids = postgres_parameter_oids(artifact, entry)?;
        let parameter_count = c_int::try_from(parameter_oids.len())
            .map_err(|_| fail("PostgreSQL parameter count exceeds i32"))?;
        let parameter_oids_pointer = if parameter_oids.is_empty() {
            std::ptr::null()
        } else {
            parameter_oids.as_ptr()
        };
        let prepared = unsafe {
            (api.prepare)(
                self.connection,
                name_c.as_ptr(),
                sql.as_ptr(),
                parameter_count,
                parameter_oids_pointer,
            )
        };
        if prepared.is_null() {
            return Err(fail("libpq returned a null prepare result"));
        }
        let prepare_status = unsafe { (api.result_status)(prepared) };
        let prepare_error = if prepare_status == 1 {
            None
        } else {
            Some(fail(
                pq_text(
                    unsafe { (api.result_error_message)(prepared) },
                    "PostgreSQL prepare error",
                )
                .unwrap_or_else(|_| {
                    format!("PostgreSQL prepare failed with status {prepare_status}")
                }),
            ))
        };
        unsafe { (api.clear)(prepared) };
        if let Some(error) = prepare_error {
            return Err(error);
        }

        let result = (|| {
            let described = unsafe { (api.describe_prepared)(self.connection, name_c.as_ptr()) };
            if described.is_null() {
                return Err(fail("libpq returned a null describe result"));
            }
            let decoded = (|| {
                let status = unsafe { (api.result_status)(described) };
                if status != 1 {
                    return Err(fail(
                        pq_text(
                            unsafe { (api.result_error_message)(described) },
                            "PostgreSQL describe error",
                        )
                        .unwrap_or_else(|_| {
                            format!("PostgreSQL describe failed with status {status}")
                        }),
                    ));
                }
                let parameter_count = unsafe { (api.nparams)(described) };
                let column_count = unsafe { (api.nfields)(described) };
                if parameter_count < 0 || column_count < 0 {
                    return Err(fail("PostgreSQL describe returned a negative count"));
                }
                let parameter_capacity = usize::try_from(parameter_count)
                    .map_err(|_| fail("PostgreSQL parameter count exceeds usize"))?;
                let mut parameter_oids = Vec::with_capacity(parameter_capacity);
                for index in 0..parameter_count {
                    parameter_oids.push(unsafe { (api.paramtype)(described, index) });
                }
                let column_capacity = usize::try_from(column_count)
                    .map_err(|_| fail("PostgreSQL column count exceeds usize"))?;
                let mut column_data = Vec::with_capacity(column_capacity);
                for index in 0..column_count {
                    column_data.push((
                        pq_text(
                            unsafe { (api.fname)(described, index) },
                            "PostgreSQL column name",
                        )?,
                        unsafe { (api.ftype)(described, index) },
                        unsafe { (api.ftable)(described, index) },
                        unsafe { (api.ftablecol)(described, index) },
                    ));
                }
                Ok((parameter_oids, column_data))
            })();
            unsafe { (api.clear)(described) };
            decoded
        })();

        let decoded = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = self.deallocate(&name);
                return Err(error);
            }
        };
        let final_result = (|| {
            let mut parameters = Vec::with_capacity(decoded.0.len());
            for (index, oid) in decoded.0.into_iter().enumerate() {
                parameters.push(NativeParameterDescription {
                    ordinal: u32::try_from(index + 1)
                        .map_err(|_| fail("PostgreSQL parameter ordinal overflow"))?,
                    source_name: None,
                    native_type: Some(self.type_name(oid)?),
                    native_type_id: Some(i64::from(oid)),
                });
            }
            let mut columns = Vec::with_capacity(decoded.1.len());
            for (index, (alias, oid, table_oid, attribute)) in decoded.1.into_iter().enumerate() {
                let (origin_schema, origin_table, origin_column) =
                    self.origin(table_oid, attribute)?;
                columns.push(NativeColumnDescription {
                    ordinal: u32::try_from(index)
                        .map_err(|_| fail("PostgreSQL column ordinal overflow"))?,
                    source_alias: alias,
                    native_type: Some(self.type_name(oid)?),
                    native_type_id: Some(i64::from(oid)),
                    origin_schema,
                    origin_table,
                    origin_column,
                    nullable: MetaNullability::Unknown,
                });
            }
            if matches!(artifact, StaticArtifact::Command(_)) && !columns.is_empty() {
                return Err(fail(
                    "PostgreSQL statement result kind disagrees with the static descriptor",
                ));
            }
            Ok(NativeStatementDescription {
                parameters,
                columns,
            })
        })();
        let cleanup = self.deallocate(&name);
        match (final_result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(description), Ok(())) => Ok(description),
        }
    }
}

impl Drop for PostgresDescriber {
    fn drop(&mut self) {
        self.close();
    }
}
