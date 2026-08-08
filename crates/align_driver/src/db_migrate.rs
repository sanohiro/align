//! Explicit Q5a/D11 SQL migration state machine.
//!
//! Normal compilation never imports or calls this module. The CLI validates and screens the whole
//! catalog before selecting one native adapter. Driver code owns native locking, history storage,
//! and SQL execution; this module owns the shared policy, reconciliation, and exact output shape.

use crate::db_prepare::{MigrationCatalog, MigrationEntry};
use align_interface::Hash128;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationDriver {
    Sqlite,
    Postgres,
}

impl MigrationDriver {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationPolicy {
    Required,
    Forbidden,
}

impl MigrationPolicy {
    pub fn tag(self) -> u8 {
        match self {
            Self::Required => 0,
            Self::Forbidden => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Forbidden => "forbidden",
        }
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Required),
            1 => Some(Self::Forbidden),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoredMigrationState {
    Applying,
    Applied,
    Failed,
}

impl StoredMigrationState {
    pub fn tag(self) -> u8 {
        match self {
            Self::Applying => 0,
            Self::Applied => 1,
            Self::Failed => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            0 => Some(Self::Applying),
            1 => Some(Self::Applied),
            2 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenedMigration {
    pub version: u32,
    pub filename: String,
    pub checksum: String,
    pub policy: MigrationPolicy,
    pub statement_count: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenedCatalog {
    pub entries: Vec<ScreenedMigration>,
    pub encoded: Vec<u8>,
    pub fingerprint: Hash128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub format_version: u32,
    pub version: u32,
    pub filename: String,
    pub checksum: String,
    pub policy: MigrationPolicy,
    pub state: StoredMigrationState,
    pub completed_statements: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationStatus {
    Pending,
    Applied,
    NameMismatch,
    ChecksumMismatch,
    PolicyMismatch,
    DirtyApplying,
    DirtyFailed,
    HistoryOnly,
}

impl MigrationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::NameMismatch => "name_mismatch",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::PolicyMismatch => "policy_mismatch",
            Self::DirtyApplying => "dirty_applying",
            Self::DirtyFailed => "dirty_failed",
            Self::HistoryOnly => "history_only",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationStatusRow {
    pub version: u32,
    pub catalog: Option<ScreenedMigration>,
    pub history: Option<HistoryRow>,
    pub status: MigrationStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub driver: MigrationDriver,
    pub rows: Vec<MigrationStatusRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationError(pub String);

impl std::fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MigrationError {}

fn fail(reason: impl Into<String>) -> MigrationError {
    MigrationError(reason.into())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedMigrationPaths {
    pub entry: PathBuf,
    pub project_root: PathBuf,
    pub migrations: PathBuf,
}

/// Validate the explicit entry and project-relative migration directory before catalog reads.
pub fn resolve_migration_paths(
    entry: &Path,
    migrations: &Path,
) -> Result<ResolvedMigrationPaths, MigrationError> {
    let lexical_entry = if entry.is_absolute() {
        entry.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| fail(format!("cannot resolve current directory: {error}")))?
            .join(entry)
    };
    let entry_metadata = std::fs::symlink_metadata(&lexical_entry).map_err(|error| {
        fail(format!(
            "cannot inspect migration entry `{}`: {error}",
            lexical_entry.display()
        ))
    })?;
    if entry_metadata.file_type().is_symlink() || !entry_metadata.file_type().is_file() {
        return Err(fail(format!(
            "migration entry `{}` must be a regular non-symlink file",
            lexical_entry.display()
        )));
    }
    if lexical_entry.extension().and_then(|value| value.to_str()) != Some("align") {
        return Err(fail(format!(
            "migration entry `{}` must have extension .align",
            lexical_entry.display()
        )));
    }
    let entry = std::fs::canonicalize(&lexical_entry).map_err(|error| {
        fail(format!(
            "cannot resolve migration entry `{}`: {error}",
            lexical_entry.display()
        ))
    })?;
    let project_root = lexical_entry
        .parent()
        .ok_or_else(|| fail("migration entry has no lexical project root"))?
        .to_path_buf();
    let physical_project_root = std::fs::canonicalize(&project_root).map_err(|error| {
        fail(format!(
            "cannot resolve migration project root `{}`: {error}",
            project_root.display()
        ))
    })?;
    if migrations.is_absolute()
        || migrations.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(fail(
            "migration directory must be a project-root-relative path without `..`",
        ));
    }
    let mut candidate = project_root.clone();
    for component in migrations.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(component) => candidate.push(component),
            _ => {
                return Err(fail(
                    "migration directory contains an invalid path component",
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(&candidate).map_err(|error| {
            fail(format!(
                "cannot inspect migration path `{}`: {error}",
                candidate.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(fail(format!(
                "migration path `{}` is a symlink",
                candidate.display()
            )));
        }
    }
    let resolved = std::fs::canonicalize(&candidate).map_err(|error| {
        fail(format!(
            "cannot resolve migration directory `{}`: {error}",
            candidate.display()
        ))
    })?;
    if !resolved.starts_with(&physical_project_root) {
        return Err(fail("migration directory escapes the project root"));
    }
    let metadata = std::fs::metadata(&resolved).map_err(|error| {
        fail(format!(
            "cannot inspect migration directory `{}`: {error}",
            resolved.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(fail(format!(
            "migration path `{}` is not a directory",
            resolved.display()
        )));
    }
    Ok(ResolvedMigrationPaths {
        entry,
        project_root,
        migrations: resolved,
    })
}

pub fn resolve_sqlite_target(project_root: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        project_root.join(target)
    }
}

const FORBIDDEN_DIRECTIVE: &[u8] = b"-- align:migration transaction=forbidden";
const REQUIRED_DIRECTIVE: &[u8] = b"-- align:migration transaction=required";

fn first_line(bytes: &[u8]) -> &[u8] {
    bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes, |end| &bytes[..end])
}

fn policy(bytes: &[u8], filename: &str) -> Result<MigrationPolicy, MigrationError> {
    let first = first_line(bytes);
    let selected = if first == FORBIDDEN_DIRECTIVE {
        MigrationPolicy::Forbidden
    } else {
        MigrationPolicy::Required
    };
    let mut recognized = 0usize;
    for line in bytes.split(|byte| *byte == b'\n') {
        if matches!(line, REQUIRED_DIRECTIVE | FORBIDDEN_DIRECTIVE) {
            recognized += 1;
        }
    }
    if recognized > usize::from(matches!(first, REQUIRED_DIRECTIVE | FORBIDDEN_DIRECTIVE)) {
        return Err(fail(format!(
            "migration `{filename}` has a migration directive after its first physical line"
        )));
    }
    if recognized > 1 {
        return Err(fail(format!(
            "migration `{filename}` has more than one migration directive"
        )));
    }
    Ok(selected)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LexState<'a> {
    Normal,
    Single { backslash_escapes: bool },
    Double,
    LineComment,
    BlockComment(u32),
    Dollar(&'a [u8]),
}

fn dollar_tag(bytes: &[u8], start: usize) -> Option<&[u8]> {
    if bytes.get(start) != Some(&b'$') {
        return None;
    }
    let mut end = start + 1;
    while let Some(byte) = bytes.get(end) {
        if *byte == b'$' {
            return Some(&bytes[start..=end]);
        }
        if !(byte.is_ascii_alphanumeric() || *byte == b'_')
            || (end == start + 1 && byte.is_ascii_digit())
        {
            return None;
        }
        end += 1;
    }
    None
}

fn postgres_statement_ranges(bytes: &[u8]) -> Result<Vec<std::ops::Range<usize>>, MigrationError> {
    let mut ranges = Vec::new();
    let mut state = LexState::Normal;
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        match state {
            LexState::Normal => match bytes[index] {
                b'\'' => {
                    let backslash_escapes = index > 0
                        && matches!(bytes[index - 1], b'e' | b'E')
                        && (index == 1
                            || !(bytes[index - 2].is_ascii_alphanumeric()
                                || bytes[index - 2] == b'_'));
                    state = LexState::Single { backslash_escapes };
                }
                b'"' => state = LexState::Double,
                b'-' if bytes.get(index + 1) == Some(&b'-') => {
                    state = LexState::LineComment;
                    index += 1;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    state = LexState::BlockComment(1);
                    index += 1;
                }
                b'$' => {
                    if let Some(tag) = dollar_tag(bytes, index) {
                        state = LexState::Dollar(tag);
                        index += tag.len() - 1;
                    }
                }
                b';' => {
                    if contains_sql_token(&bytes[start..index])? {
                        ranges.push(start..index + 1);
                    }
                    start = index + 1;
                }
                _ => {}
            },
            LexState::Single { backslash_escapes } => {
                if backslash_escapes && bytes[index] == b'\\' {
                    if index + 1 < bytes.len() {
                        index += 1;
                    }
                } else if bytes[index] == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 1;
                    } else {
                        state = LexState::Normal;
                    }
                }
            }
            LexState::Double => {
                if bytes[index] == b'"' {
                    if bytes.get(index + 1) == Some(&b'"') {
                        index += 1;
                    } else {
                        state = LexState::Normal;
                    }
                }
            }
            LexState::LineComment => {
                if bytes[index] == b'\n' {
                    state = LexState::Normal;
                }
            }
            LexState::BlockComment(depth) => {
                if bytes[index..].starts_with(b"/*") {
                    state = LexState::BlockComment(
                        depth
                            .checked_add(1)
                            .ok_or_else(|| fail("SQL block-comment depth overflow"))?,
                    );
                    index += 1;
                } else if bytes[index..].starts_with(b"*/") {
                    state = if depth == 1 {
                        LexState::Normal
                    } else {
                        LexState::BlockComment(depth - 1)
                    };
                    index += 1;
                }
            }
            LexState::Dollar(tag) => {
                if bytes[index..].starts_with(tag) {
                    index += tag.len() - 1;
                    state = LexState::Normal;
                }
            }
        }
        index += 1;
    }
    match state {
        LexState::Normal | LexState::LineComment => {}
        LexState::Single { .. } => return Err(fail("SQL contains an unterminated string literal")),
        LexState::Double => return Err(fail("SQL contains an unterminated quoted identifier")),
        LexState::BlockComment(_) => {
            return Err(fail("SQL contains an unterminated block comment"));
        }
        LexState::Dollar(_) => return Err(fail("SQL contains an unterminated dollar-quoted body")),
    }
    if contains_sql_token(&bytes[start..])? {
        ranges.push(start..bytes.len());
    }
    Ok(ranges)
}

fn contains_sql_token(bytes: &[u8]) -> Result<bool, MigrationError> {
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() || bytes[index] == b';' {
            index += 1;
        } else if bytes[index..].starts_with(b"--") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if bytes[index..].starts_with(b"/*") {
            let mut depth = 1u32;
            index += 2;
            while index < bytes.len() && depth != 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth = depth
                        .checked_add(1)
                        .ok_or_else(|| fail("SQL block-comment depth overflow"))?;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            if depth != 0 {
                return Err(fail("SQL contains an unterminated block comment"));
            }
        } else {
            return Ok(true);
        }
    }
    Ok(false)
}

fn leading_words(bytes: &[u8]) -> Result<Vec<String>, MigrationError> {
    let mut words = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() && words.len() < 5 {
        if bytes[index].is_ascii_whitespace() || bytes[index] == b';' {
            index += 1;
        } else if bytes[index..].starts_with(b"--") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if bytes[index..].starts_with(b"/*") {
            let mut depth = 1u32;
            index += 2;
            while index < bytes.len() && depth != 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth = depth
                        .checked_add(1)
                        .ok_or_else(|| fail("SQL block-comment depth overflow"))?;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            if depth != 0 {
                return Err(fail("SQL contains an unterminated block comment"));
            }
        } else if bytes[index].is_ascii_alphabetic() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            words.push(String::from_utf8_lossy(&bytes[start..index]).to_ascii_uppercase());
        } else {
            break;
        }
    }
    Ok(words)
}

fn rejects_transaction_control(bytes: &[u8]) -> Result<bool, MigrationError> {
    let words = leading_words(bytes)?;
    Ok(matches!(
        words.first().map(String::as_str),
        Some("BEGIN" | "COMMIT" | "END" | "ROLLBACK" | "ABORT" | "SAVEPOINT" | "RELEASE")
    ) || matches!(words.as_slice(), [first, second, ..] if matches!(first.as_str(), "START" | "PREPARE" | "SET") && second == "TRANSACTION")
        || matches!(words.as_slice(), [first, second, third, fourth, fifth, ..] if first == "SET" && second == "SESSION" && third == "CHARACTERISTICS" && fourth == "AS" && fifth == "TRANSACTION"))
}

fn screen_ranges(
    entry: &MigrationEntry,
    policy: MigrationPolicy,
    ranges: Vec<std::ops::Range<usize>>,
) -> Result<ScreenedMigration, MigrationError> {
    if ranges.is_empty() {
        return Err(fail(format!(
            "migration `{}` contains no SQL statement",
            entry.filename
        )));
    }
    for range in &ranges {
        if rejects_transaction_control(&entry.bytes[range.clone()])? {
            return Err(fail(format!(
                "migration `{}` contains a transaction-control statement",
                entry.filename
            )));
        }
    }
    let statement_count = u32::try_from(ranges.len()).map_err(|_| {
        fail(format!(
            "migration `{}` has too many statements",
            entry.filename
        ))
    })?;
    if policy == MigrationPolicy::Forbidden && statement_count != 1 {
        return Err(fail(format!(
            "forbidden migration `{}` must contain exactly one statement",
            entry.filename
        )));
    }
    Ok(ScreenedMigration {
        version: entry.version,
        filename: entry.filename.clone(),
        checksum: Hash128::of(&entry.bytes).to_hex(),
        policy,
        statement_count,
        bytes: entry.bytes.clone(),
    })
}

pub fn screen_postgres_catalog(
    catalog: &MigrationCatalog,
) -> Result<ScreenedCatalog, MigrationError> {
    let mut entries = Vec::with_capacity(catalog.entries.len());
    for entry in &catalog.entries {
        let policy = policy(&entry.bytes, &entry.filename)?;
        entries.push(screen_ranges(
            entry,
            policy,
            postgres_statement_ranges(&entry.bytes)?,
        )?);
    }
    Ok(ScreenedCatalog {
        entries,
        encoded: catalog.encoded.clone(),
        fingerprint: catalog.fingerprint,
    })
}

pub fn screen_sqlite_catalog(
    catalog: &MigrationCatalog,
    mut complete: impl FnMut(&[u8]) -> Result<bool, MigrationError>,
) -> Result<ScreenedCatalog, MigrationError> {
    let mut entries = Vec::with_capacity(catalog.entries.len());
    for entry in &catalog.entries {
        let policy = policy(&entry.bytes, &entry.filename)?;
        let mut ranges = Vec::new();
        let mut start = 0usize;
        for (index, byte) in entry.bytes.iter().enumerate() {
            if *byte == b';' && complete(&entry.bytes[start..=index])? {
                if contains_sql_token(&entry.bytes[start..index])? {
                    ranges.push(start..index + 1);
                }
                start = index + 1;
            }
        }
        if contains_sql_token(&entry.bytes[start..])? {
            let mut terminated = entry.bytes[start..].to_vec();
            terminated.push(b';');
            if !complete(&terminated)? {
                return Err(fail(format!(
                    "migration `{}` contains incomplete SQLite SQL",
                    entry.filename
                )));
            }
            ranges.push(start..entry.bytes.len());
        } else if !entry.bytes[start..].is_empty() && !complete(&entry.bytes[start..])? {
            return Err(fail(format!(
                "migration `{}` contains incomplete SQLite SQL",
                entry.filename
            )));
        }
        entries.push(screen_ranges(entry, policy, ranges)?);
    }
    Ok(ScreenedCatalog {
        entries,
        encoded: catalog.encoded.clone(),
        fingerprint: catalog.fingerprint,
    })
}

fn canonical_checksum(value: &str) -> bool {
    value.len() == 32
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn filename_version(value: &str) -> Option<u32> {
    let bytes = value.as_bytes();
    let prefix = bytes.get(..4)?;
    if bytes.len() < 10
        || bytes.get(4) != Some(&b'_')
        || !value.ends_with(".sql")
        || !prefix.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let stem = &bytes[5..bytes.len() - 4];
    if stem.is_empty()
        || !stem[0].is_ascii_lowercase()
        || !stem[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return None;
    }
    std::str::from_utf8(prefix).ok()?.parse().ok()
}

pub fn reconcile(
    driver: MigrationDriver,
    catalog: &ScreenedCatalog,
    mut history: Vec<HistoryRow>,
) -> Result<MigrationReport, MigrationError> {
    history.sort_by_key(|row| row.version);
    let mut prior = None;
    for row in &history {
        if row.format_version != 1 || !(1..=9999).contains(&row.version) {
            return Err(fail("migration history contains an invalid format/version"));
        }
        if prior == Some(row.version) {
            return Err(fail(format!(
                "migration history repeats version {:04}",
                row.version
            )));
        }
        prior = Some(row.version);
        if filename_version(&row.filename) != Some(row.version) {
            return Err(fail(format!(
                "migration history version {:04} has an invalid filename",
                row.version
            )));
        }
        if !canonical_checksum(&row.checksum) {
            return Err(fail(format!(
                "migration history version {:04} has an invalid checksum",
                row.version
            )));
        }
        match (row.policy, row.state, row.completed_statements) {
            (
                MigrationPolicy::Forbidden,
                StoredMigrationState::Applying | StoredMigrationState::Failed,
                0,
            ) => {}
            (MigrationPolicy::Forbidden, StoredMigrationState::Applied, 1) => {}
            (MigrationPolicy::Required, StoredMigrationState::Applied, count) if count != 0 => {}
            _ => {
                return Err(fail(format!(
                    "migration history version {:04} has an invalid state",
                    row.version
                )));
            }
        }
    }

    let mut rows = Vec::with_capacity(catalog.entries.len() + history.len());
    let mut history_index = 0usize;
    for current in &catalog.entries {
        while history
            .get(history_index)
            .is_some_and(|row| row.version < current.version)
        {
            let stored = history[history_index].clone();
            rows.push(MigrationStatusRow {
                version: stored.version,
                catalog: None,
                history: Some(stored),
                status: MigrationStatus::HistoryOnly,
            });
            history_index += 1;
        }
        let stored = history
            .get(history_index)
            .filter(|row| row.version == current.version)
            .cloned();
        if stored.is_some() {
            history_index += 1;
        }
        if let Some(stored) = &stored
            && stored.state == StoredMigrationState::Applied
            && stored.completed_statements != current.statement_count
        {
            return Err(fail(format!(
                "migration history version {:04} has an invalid completed-statement count",
                current.version
            )));
        }
        let status = match &stored {
            None => MigrationStatus::Pending,
            Some(row) if row.filename != current.filename => MigrationStatus::NameMismatch,
            Some(row) if row.checksum != current.checksum => MigrationStatus::ChecksumMismatch,
            Some(row) if row.policy != current.policy => MigrationStatus::PolicyMismatch,
            Some(row) if row.state == StoredMigrationState::Applying => {
                MigrationStatus::DirtyApplying
            }
            Some(row) if row.state == StoredMigrationState::Failed => MigrationStatus::DirtyFailed,
            Some(_) => MigrationStatus::Applied,
        };
        rows.push(MigrationStatusRow {
            version: current.version,
            catalog: Some(current.clone()),
            history: stored,
            status,
        });
    }
    for stored in history.into_iter().skip(history_index) {
        rows.push(MigrationStatusRow {
            version: stored.version,
            catalog: None,
            history: Some(stored),
            status: MigrationStatus::HistoryOnly,
        });
    }
    rows.sort_by(
        |left, right| match (left.catalog.is_some(), right.catalog.is_some()) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.version.cmp(&right.version),
        },
    );
    Ok(MigrationReport { driver, rows })
}

impl MigrationReport {
    pub fn is_current(&self) -> bool {
        self.rows
            .iter()
            .all(|row| row.status == MigrationStatus::Applied)
    }

    pub fn can_migrate(&self) -> bool {
        let mut pending = false;
        for row in &self.rows {
            match row.status {
                MigrationStatus::Applied if !pending => {}
                MigrationStatus::Pending => pending = true,
                _ => return false,
            }
        }
        true
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        for row in &self.rows {
            let (catalog_name, catalog_checksum, catalog_policy) =
                row.catalog.as_ref().map_or(("-", "-", "-"), |value| {
                    (
                        value.filename.as_str(),
                        value.checksum.as_str(),
                        value.policy.label(),
                    )
                });
            let (history_name, history_checksum, history_policy, history_state, history_completed) =
                row.history
                    .as_ref()
                    .map_or(("-", "-", "-", "-", "-".to_string()), |value| {
                        (
                            value.filename.as_str(),
                            value.checksum.as_str(),
                            value.policy.label(),
                            value.state.label(),
                            value.completed_statements.to_string(),
                        )
                    });
            output.push_str(&format!(
                "migration version={:04} catalog_name={catalog_name} catalog_checksum={catalog_checksum} catalog_policy={catalog_policy} history_name={history_name} history_checksum={history_checksum} history_policy={history_policy} history_state={history_state} history_completed={history_completed} state={}\n",
                row.version,
                row.status.label(),
            ));
        }
        let applied = self
            .rows
            .iter()
            .filter(|row| row.status == MigrationStatus::Applied)
            .count();
        let pending = self
            .rows
            .iter()
            .filter(|row| row.status == MigrationStatus::Pending)
            .count();
        let dirty = self
            .rows
            .iter()
            .filter(|row| {
                matches!(
                    row.status,
                    MigrationStatus::DirtyApplying | MigrationStatus::DirtyFailed
                )
            })
            .count();
        let mismatched = self
            .rows
            .iter()
            .filter(|row| {
                matches!(
                    row.status,
                    MigrationStatus::NameMismatch
                        | MigrationStatus::ChecksumMismatch
                        | MigrationStatus::PolicyMismatch
                )
            })
            .count();
        let history_only = self
            .rows
            .iter()
            .filter(|row| row.status == MigrationStatus::HistoryOnly)
            .count();
        output.push_str(&format!(
            "summary driver={} applied={applied} pending={pending} dirty={dirty} mismatched={mismatched} history_only={history_only}\n",
            self.driver.label(),
        ));
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: u32, sql: &str) -> MigrationEntry {
        MigrationEntry {
            version,
            filename: format!("{version:04}_migration.sql"),
            path: std::path::PathBuf::from(format!("{version:04}_migration.sql")),
            bytes: sql.as_bytes().to_vec(),
        }
    }

    #[test]
    fn postgres_screening_handles_dollar_quotes_and_transaction_control() {
        let catalog = crate::db_prepare::encode_migration_catalog(vec![entry(
            1,
            "CREATE FUNCTION f() RETURNS void AS $$ BEGIN PERFORM 1; END $$ LANGUAGE plpgsql;\nSELECT 2",
        )])
        .expect("catalog");
        let screened = screen_postgres_catalog(&catalog).expect("screen");
        assert_eq!(screened.entries[0].statement_count, 2);

        let catalog =
            crate::db_prepare::encode_migration_catalog(vec![entry(1, "/*x*/ START TRANSACTION")])
                .expect("catalog");
        assert!(
            screen_postgres_catalog(&catalog)
                .unwrap_err()
                .0
                .contains("transaction-control")
        );
    }

    #[test]
    fn postgres_screening_handles_escape_strings_and_nested_leading_comments() {
        let catalog = crate::db_prepare::encode_migration_catalog(vec![entry(
            1,
            "SELECT E'escaped \\\' ; still text'; /* outer /* inner */ tail */ SELECT 2;",
        )])
        .unwrap();
        assert_eq!(
            screen_postgres_catalog(&catalog).unwrap().entries[0].statement_count,
            2
        );

        let rejected = crate::db_prepare::encode_migration_catalog(vec![entry(
            1,
            "/* outer /* inner */ tail */ START TRANSACTION;",
        )])
        .unwrap();
        assert!(
            screen_postgres_catalog(&rejected)
                .unwrap_err()
                .to_string()
                .contains("transaction-control")
        );
    }

    #[test]
    fn reconciliation_uses_exact_precedence_and_output_provenance() {
        let catalog = crate::db_prepare::encode_migration_catalog(vec![entry(1, "SELECT 1")])
            .expect("catalog");
        let screened = screen_postgres_catalog(&catalog).expect("screen");
        let report = reconcile(
            MigrationDriver::Postgres,
            &screened,
            vec![HistoryRow {
                format_version: 1,
                version: 1,
                filename: "0001_other.sql".to_string(),
                checksum: "00000000000000000000000000000000".to_string(),
                policy: MigrationPolicy::Forbidden,
                state: StoredMigrationState::Failed,
                completed_statements: 0,
            }],
        )
        .expect("report");
        assert_eq!(report.rows[0].status, MigrationStatus::NameMismatch);
        let output = report.render();
        assert!(output.contains("catalog_name=0001_migration.sql"));
        assert!(output.contains("history_name=0001_other.sql"));
        assert!(output.contains("history_completed=0 state=name_mismatch"));
    }
}
