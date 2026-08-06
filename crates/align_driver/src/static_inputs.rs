//! Deterministic compiler-registered static inputs (L5b).
//!
//! This module is the driver-side boundary between resolved static constructors and the L5a
//! artifact codec. It deliberately does not discover constructors or scan a directory. A future
//! frontend supplies the resolved descriptor identity, source literal, and source/import digest;
//! this module then resolves one exact file (or keeps decoded inline bytes), snapshots the exact
//! checked-metadata paths, and produces a fail-closed manifest/action digest.

use align_interface::{Driver, DriverRestriction, Hash128, SqlSourceIdentity};
use align_span::{FileId, SourceMap};
use std::cmp::Ordering;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const STATIC_INPUT_MANIFEST_FORMAT_VERSION: u32 = 1;
pub const STATIC_INPUT_MANIFEST_MAGIC: [u8; 8] = *b"ALIGNINP";
const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SEQUENCE: usize = 1 << 16;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StaticConsumerKind {
    Query = 0,
    Command = 1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataState {
    Missing,
    Present {
        content_hash: Hash128,
        format_version: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedMetadataInput {
    pub driver: Driver,
    pub logical_path: String,
    pub state: MetadataState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticInput {
    pub descriptor_id: String,
    pub source: SqlSourceIdentity,
    pub content_hash: Hash128,
    pub consumer_kind: StaticConsumerKind,
    pub driver_restriction: DriverRestriction,
    pub checked_metadata: Vec<CheckedMetadataInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedStaticInput {
    pub input: StaticInput,
    pub bytes: Vec<u8>,
    pub source_map_file: Option<FileId>,
    pub resolved_path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticInputManifest {
    pub resolution_digest: Hash128,
    pub inputs: Vec<StaticInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaticInputError {
    InvalidDescriptorId,
    InvalidSource(String),
    InvalidPath(String),
    RootNotDirectory(PathBuf),
    OutsideProjectRoot(PathBuf),
    MissingFile(PathBuf),
    NotRegularFile(PathBuf),
    Io { path: PathBuf, message: String },
    InvalidUtf8 { logical_path: String },
    EmbeddedNul { logical_path: String, offset: u32 },
    HashMismatch { logical_path: String },
    Stale(String),
    MetadataMalformed { logical_path: String },
    BadMagic,
    UnknownVersion(u32),
    Truncated,
    BadTag { what: &'static str, tag: u8 },
    BadUtf8,
    TrailingBytes,
    NonCanonical(String),
}

impl std::fmt::Display for StaticInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDescriptorId => write!(f, "static descriptor id is empty or contains NUL"),
            Self::InvalidSource(reason) => write!(f, "invalid static source: {reason}"),
            Self::InvalidPath(reason) => write!(f, "invalid static input path: {reason}"),
            Self::RootNotDirectory(path) => {
                write!(f, "project root is not a directory: {}", path.display())
            }
            Self::OutsideProjectRoot(path) => {
                write!(
                    f,
                    "static input escapes the project root: {}",
                    path.display()
                )
            }
            Self::MissingFile(path) => {
                write!(f, "static input file does not exist: {}", path.display())
            }
            Self::NotRegularFile(path) => {
                write!(f, "static input is not a regular file: {}", path.display())
            }
            Self::Io { path, message } => {
                write!(f, "cannot read static input {}: {message}", path.display())
            }
            Self::InvalidUtf8 { logical_path } => {
                write!(f, "static input is not UTF-8: {logical_path}")
            }
            Self::EmbeddedNul {
                logical_path,
                offset,
            } => {
                write!(
                    f,
                    "static input contains U+0000 at byte {offset}: {logical_path}"
                )
            }
            Self::HashMismatch { logical_path } => {
                write!(f, "static input content changed: {logical_path}")
            }
            Self::Stale(reason) => write!(f, "static input manifest is stale: {reason}"),
            Self::MetadataMalformed { logical_path } => {
                write!(f, "checked metadata is malformed: {logical_path}")
            }
            Self::BadMagic => write!(f, "static input manifest has an invalid magic"),
            Self::UnknownVersion(version) => {
                write!(f, "unknown static input manifest version {version}")
            }
            Self::Truncated => write!(f, "static input manifest is truncated"),
            Self::BadTag { what, tag } => write!(f, "invalid {what} tag byte {tag}"),
            Self::BadUtf8 => write!(f, "static input manifest contains invalid UTF-8"),
            Self::TrailingBytes => write!(f, "static input manifest has trailing bytes"),
            Self::NonCanonical(reason) => {
                write!(f, "non-canonical static input manifest: {reason}")
            }
        }
    }
}

impl std::error::Error for StaticInputError {}

fn invalid_descriptor_id(id: &str) -> bool {
    id.is_empty() || id.as_bytes().contains(&0) || id.contains('\n') || id.contains('\r')
}

fn validate_descriptor_id(id: &str) -> Result<(), StaticInputError> {
    if invalid_descriptor_id(id) {
        Err(StaticInputError::InvalidDescriptorId)
    } else if id.len() > MAX_FIELD_BYTES {
        Err(StaticInputError::NonCanonical(
            "descriptor id exceeds the field limit".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn validate_text_field(value: &str, what: &str) -> Result<(), StaticInputError> {
    if value.as_bytes().contains(&0) {
        return Err(StaticInputError::NonCanonical(format!(
            "{what} contains U+0000"
        )));
    }
    if value.len() > MAX_FIELD_BYTES {
        return Err(StaticInputError::NonCanonical(format!(
            "{what} exceeds the field limit"
        )));
    }
    Ok(())
}

fn source_sort_key(source: &SqlSourceIdentity) -> (u8, &[u8]) {
    match source {
        SqlSourceIdentity::File { logical_path } => (0, logical_path.as_bytes()),
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => (1, query_or_command_id.as_bytes()),
    }
}

fn source_identity_id(source: &SqlSourceIdentity) -> &str {
    match source {
        SqlSourceIdentity::File { logical_path } => logical_path,
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => query_or_command_id,
    }
}

fn source_identity_path(source: &SqlSourceIdentity) -> Option<&str> {
    match source {
        SqlSourceIdentity::File { logical_path } => Some(logical_path),
        SqlSourceIdentity::Inline { .. } => None,
    }
}

fn driver_dir(driver: Driver) -> &'static str {
    match driver {
        Driver::SQLite => "sqlite",
        Driver::PostgreSQL => "postgres",
    }
}

pub fn metadata_logical_path(
    descriptor_id: &str,
    driver: Driver,
) -> Result<String, StaticInputError> {
    validate_descriptor_id(descriptor_id)?;
    let digest = Hash128::of(descriptor_id.as_bytes()).to_hex();
    Ok(format!(".align-db/{}/{digest}.json", driver_dir(driver)))
}

pub fn metadata_path(
    project_root: &Path,
    descriptor_id: &str,
    driver: Driver,
) -> Result<PathBuf, StaticInputError> {
    let logical = metadata_logical_path(descriptor_id, driver)?;
    Ok(project_root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn canonical_root(project_root: &Path) -> Result<PathBuf, StaticInputError> {
    let root = fs::canonicalize(project_root).map_err(|e| StaticInputError::Io {
        path: project_root.to_path_buf(),
        message: e.to_string(),
    })?;
    if !root.is_dir() {
        return Err(StaticInputError::RootNotDirectory(root));
    }
    Ok(root)
}

fn ensure_inside(root: &Path, path: &Path) -> Result<(), StaticInputError> {
    if path.strip_prefix(root).is_err() {
        Err(StaticInputError::OutsideProjectRoot(path.to_path_buf()))
    } else {
        Ok(())
    }
}

fn canonical_defining_file(
    root: &Path,
    defining_align_file: &Path,
) -> Result<PathBuf, StaticInputError> {
    let candidate = if defining_align_file.is_absolute() {
        defining_align_file.to_path_buf()
    } else {
        root.join(defining_align_file)
    };
    let canonical = fs::canonicalize(&candidate).map_err(|e| StaticInputError::Io {
        path: candidate.clone(),
        message: e.to_string(),
    })?;
    ensure_inside(root, &canonical)?;
    if !canonical.is_file() {
        return Err(StaticInputError::NotRegularFile(canonical));
    }
    Ok(canonical)
}

fn validate_literal_path(raw: &str) -> Result<(), StaticInputError> {
    if raw.is_empty() {
        return Err(StaticInputError::InvalidPath(
            "explicit path is empty".to_string(),
        ));
    }
    if raw.as_bytes().contains(&0) {
        return Err(StaticInputError::InvalidPath(
            "path contains NUL".to_string(),
        ));
    }
    if raw.contains('\\') {
        return Err(StaticInputError::InvalidPath(
            "backslash is not a portable separator".to_string(),
        ));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(StaticInputError::InvalidPath(
            "absolute paths are not allowed".to_string(),
        ));
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(StaticInputError::InvalidPath(
                "path escapes its defining directory".to_string(),
            ));
        }
    }
    Ok(())
}

fn logical_path(root: &Path, candidate: &Path) -> Result<String, StaticInputError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| StaticInputError::OutsideProjectRoot(candidate.to_path_buf()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    StaticInputError::InvalidPath("logical path is not UTF-8".to_string())
                })?;
                if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
                    return Err(StaticInputError::InvalidPath(
                        "logical path is not canonical".to_string(),
                    ));
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(StaticInputError::InvalidPath(
                    "logical path is not relative".to_string(),
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(StaticInputError::InvalidPath(
            "logical path is empty".to_string(),
        ));
    }
    Ok(parts.join("/"))
}

fn read_static_bytes(
    root: &Path,
    candidate: &Path,
    logical: &str,
) -> Result<Vec<u8>, StaticInputError> {
    let canonical = fs::canonicalize(candidate).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StaticInputError::MissingFile(candidate.to_path_buf())
        } else {
            StaticInputError::Io {
                path: candidate.to_path_buf(),
                message: e.to_string(),
            }
        }
    })?;
    ensure_inside(root, &canonical)?;
    if !canonical.is_file() {
        return Err(StaticInputError::NotRegularFile(canonical));
    }
    let bytes = fs::read(&canonical).map_err(|e| StaticInputError::Io {
        path: canonical.clone(),
        message: e.to_string(),
    })?;
    if std::str::from_utf8(&bytes).is_err() {
        return Err(StaticInputError::InvalidUtf8 {
            logical_path: logical.to_string(),
        });
    }
    if let Some(offset) = bytes.iter().position(|byte| *byte == 0) {
        let offset = u32::try_from(offset).map_err(|_| StaticInputError::EmbeddedNul {
            logical_path: logical.to_string(),
            offset: u32::MAX,
        })?;
        return Err(StaticInputError::EmbeddedNul {
            logical_path: logical.to_string(),
            offset,
        });
    }
    Ok(bytes)
}

fn make_input(
    descriptor_id: String,
    source: SqlSourceIdentity,
    bytes: Vec<u8>,
    consumer_kind: StaticConsumerKind,
    driver_restriction: DriverRestriction,
) -> Result<StaticInput, StaticInputError> {
    validate_descriptor_id(&descriptor_id)?;
    match &source {
        SqlSourceIdentity::File { logical_path } => {
            validate_text_field(logical_path, "file logical path")?;
            if logical_path.is_empty()
                || logical_path.starts_with('/')
                || logical_path.contains('\\')
            {
                return Err(StaticInputError::InvalidSource(
                    "file logical path is not root-relative".to_string(),
                ));
            }
            if logical_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(StaticInputError::InvalidSource(
                    "file logical path is not canonical".to_string(),
                ));
            }
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            if query_or_command_id != &descriptor_id {
                return Err(StaticInputError::InvalidSource(
                    "inline identity does not equal the descriptor id".to_string(),
                ));
            }
        }
    }
    if std::str::from_utf8(&bytes).is_err() {
        return Err(StaticInputError::InvalidUtf8 {
            logical_path: source_identity_id(&source).to_string(),
        });
    }
    if let Some(offset) = bytes.iter().position(|byte| *byte == 0) {
        let offset = u32::try_from(offset).unwrap_or(u32::MAX);
        return Err(StaticInputError::EmbeddedNul {
            logical_path: source_identity_id(&source).to_string(),
            offset,
        });
    }
    Ok(StaticInput {
        descriptor_id,
        source,
        content_hash: Hash128::of(&bytes),
        consumer_kind,
        driver_restriction,
        checked_metadata: Vec::new(),
    })
}

pub fn resolve_static_file(
    project_root: &Path,
    defining_align_file: &Path,
    path_literal: Option<&str>,
    descriptor_id: impl Into<String>,
    consumer_kind: StaticConsumerKind,
    driver_restriction: DriverRestriction,
    source_map: Option<&mut SourceMap>,
) -> Result<ResolvedStaticInput, StaticInputError> {
    let descriptor_id = descriptor_id.into();
    validate_descriptor_id(&descriptor_id)?;
    let root = canonical_root(project_root)?;
    let defining = canonical_defining_file(&root, defining_align_file)?;
    let candidate = match path_literal {
        None => defining.with_extension("sql"),
        Some(raw) => {
            validate_literal_path(raw)?;
            defining
                .parent()
                .ok_or_else(|| {
                    StaticInputError::InvalidPath("defining file has no directory".to_string())
                })?
                .join(raw)
        }
    };
    let logical = logical_path(&root, &candidate)?;
    let bytes = read_static_bytes(&root, &candidate, &logical)?;
    let input = make_input(
        descriptor_id,
        SqlSourceIdentity::File {
            logical_path: logical.clone(),
        },
        bytes.clone(),
        consumer_kind,
        driver_restriction,
    )?;
    let text = std::str::from_utf8(&bytes).map_err(|_| StaticInputError::InvalidUtf8 {
        logical_path: logical.clone(),
    })?;
    let source_map_file = source_map.map(|map| map.add_file(logical.clone(), text.to_string()));
    Ok(ResolvedStaticInput {
        input,
        bytes,
        source_map_file,
        resolved_path: Some(candidate),
    })
}

pub fn resolve_inline_static_input(
    descriptor_id: impl Into<String>,
    decoded_sql: &str,
    consumer_kind: StaticConsumerKind,
    driver_restriction: DriverRestriction,
) -> Result<ResolvedStaticInput, StaticInputError> {
    let descriptor_id = descriptor_id.into();
    let bytes = decoded_sql.as_bytes().to_vec();
    let input = make_input(
        descriptor_id.clone(),
        SqlSourceIdentity::Inline {
            query_or_command_id: descriptor_id,
        },
        bytes.clone(),
        consumer_kind,
        driver_restriction,
    )?;
    Ok(ResolvedStaticInput {
        input,
        bytes,
        source_map_file: None,
        resolved_path: None,
    })
}

pub fn snapshot_checked_metadata(
    project_root: &Path,
    descriptor_id: &str,
    driver: Driver,
) -> Result<CheckedMetadataInput, StaticInputError> {
    let logical = metadata_logical_path(descriptor_id, driver)?;
    let root = canonical_root(project_root)?;
    let path = root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR));
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {
            let bytes = read_metadata_bytes(&root, &path)?;
            let format_version = parse_metadata_format_version(&bytes, &logical)?;
            Ok(CheckedMetadataInput {
                driver,
                logical_path: logical,
                state: MetadataState::Present {
                    content_hash: Hash128::of(&bytes),
                    format_version,
                },
            })
        }
        Ok(_) => Err(StaticInputError::NotRegularFile(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_metadata_parent_inside(&root, &path)?;
            Ok(CheckedMetadataInput {
                driver,
                logical_path: logical,
                state: MetadataState::Missing,
            })
        }
        Err(error) => Err(StaticInputError::Io {
            path,
            message: error.to_string(),
        }),
    }
}

fn parse_metadata_format_version(
    bytes: &[u8],
    logical_path: &str,
) -> Result<u32, StaticInputError> {
    if bytes.len() > MAX_FIELD_BYTES {
        return Err(StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        });
    }
    std::str::from_utf8(bytes).map_err(|_| StaticInputError::MetadataMalformed {
        logical_path: logical_path.to_string(),
    })?;
    if bytes.len() < 2 || bytes.last() != Some(&b'\n') || bytes[..bytes.len() - 1].contains(&b'\n')
    {
        return Err(StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        });
    }
    let mut parser = MetadataJsonParser::new(&bytes[..bytes.len() - 1]);
    parser
        .parse_metadata_object()
        .map_err(|_| StaticInputError::MetadataMalformed {
            logical_path: logical_path.to_string(),
        })
}

const METADATA_TOP_LEVEL_KEYS: &[&str] = &[
    "format_version",
    "descriptor_id",
    "module",
    "item",
    "driver",
    "driver_restriction",
    "statement_kind",
    "statement_class",
    "source_identity",
    "source_sql_hash",
    "wire_sql_hash",
    "rewrite_format_version",
    "static_options_hash",
    "params_fingerprint",
    "row_fingerprint",
    "schema_fingerprint",
    "engine_version",
    "driver_version",
    "search_path",
    "extensions",
    "parameters",
    "columns",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MetadataJsonKind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

struct MetadataJsonParser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> MetadataJsonParser<'a> {
    const MAX_DEPTH: usize = 128;

    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn parse_metadata_object(&mut self) -> Result<u32, ()> {
        self.expect_byte(b'{')?;
        for (index, expected_key) in METADATA_TOP_LEVEL_KEYS.iter().enumerate() {
            if index != 0 {
                self.expect_byte(b',')?;
            }
            let key = self.parse_string()?;
            if key != expected_key.as_bytes() {
                return Err(());
            }
            self.expect_byte(b':')?;
            let kind = if index == 0 {
                let number = self.parse_number()?;
                if number != b"1" {
                    return Err(());
                }
                MetadataJsonKind::Number
            } else {
                self.parse_value(0)?.0
            };
            if !metadata_top_level_kind_is_valid(index, kind) {
                return Err(());
            }
        }
        self.expect_byte(b'}')?;
        if self.position != self.bytes.len() {
            return Err(());
        }
        Ok(1)
    }

    fn parse_value(&mut self, depth: usize) -> Result<(MetadataJsonKind, Option<&'a [u8]>), ()> {
        if depth > Self::MAX_DEPTH {
            return Err(());
        }
        let byte = *self.bytes.get(self.position).ok_or(())?;
        match byte {
            b'{' => {
                self.parse_object(depth + 1)?;
                Ok((MetadataJsonKind::Object, None))
            }
            b'[' => {
                self.parse_array(depth + 1)?;
                Ok((MetadataJsonKind::Array, None))
            }
            b'"' => Ok((MetadataJsonKind::String, Some(self.parse_string()?))),
            b'-' | b'0'..=b'9' => Ok((MetadataJsonKind::Number, Some(self.parse_number()?))),
            b't' => {
                self.expect_bytes(b"true")?;
                Ok((MetadataJsonKind::Bool, None))
            }
            b'f' => {
                self.expect_bytes(b"false")?;
                Ok((MetadataJsonKind::Bool, None))
            }
            b'n' => {
                self.expect_bytes(b"null")?;
                Ok((MetadataJsonKind::Null, None))
            }
            _ => Err(()),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<(), ()> {
        self.expect_byte(b'{')?;
        let mut keys: Vec<&[u8]> = Vec::new();
        if self.peek_byte() == Some(b'}') {
            self.position += 1;
            return Ok(());
        }
        loop {
            let key = self.parse_string()?;
            if keys.contains(&key) || keys.len() >= MAX_SEQUENCE {
                return Err(());
            }
            keys.push(key);
            self.expect_byte(b':')?;
            self.parse_value(depth)?;
            match self.peek_byte() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(());
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<(), ()> {
        self.expect_byte(b'[')?;
        if self.peek_byte() == Some(b']') {
            self.position += 1;
            return Ok(());
        }
        let mut count = 0usize;
        loop {
            if count >= MAX_SEQUENCE {
                return Err(());
            }
            self.parse_value(depth)?;
            count += 1;
            match self.peek_byte() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(());
                }
                _ => return Err(()),
            }
        }
    }

    fn parse_string(&mut self) -> Result<&'a [u8], ()> {
        self.expect_byte(b'"')?;
        let start = self.position;
        loop {
            let byte = *self.bytes.get(self.position).ok_or(())?;
            match byte {
                b'"' => {
                    let end = self.position;
                    self.position += 1;
                    return Ok(&self.bytes[start..end]);
                }
                b'\\' => {
                    self.position += 1;
                    match self.bytes.get(self.position).copied() {
                        Some(b'"' | b'\\' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.position += 1;
                        }
                        Some(b'u') => {
                            let start_escape = self.position;
                            let digits = self
                                .bytes
                                .get(start_escape + 1..start_escape + 5)
                                .ok_or(())?;
                            if digits.len() != 4
                                || !digits.iter().all(|digit| digit.is_ascii_hexdigit())
                                || digits.iter().any(|digit| digit.is_ascii_uppercase())
                            {
                                return Err(());
                            }
                            let value = digits.iter().try_fold(0u32, |value, digit| {
                                Some(value * 16 + hex_value(*digit)?)
                            });
                            if value.ok_or(())? > 0x1f {
                                return Err(());
                            }
                            self.position += 5;
                        }
                        _ => return Err(()),
                    }
                }
                0x00..=0x1f => return Err(()),
                _ => self.position += 1,
            }
        }
    }

    fn parse_number(&mut self) -> Result<&'a [u8], ()> {
        let start = self.position;
        if self.peek_byte() == Some(b'-') {
            self.position += 1;
        }
        match self.peek_byte() {
            Some(b'0') => {
                self.position += 1;
                if self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
                    return Err(());
                }
            }
            Some(b'1'..=b'9') => {
                self.position += 1;
                while self.peek_byte().is_some_and(|byte| byte.is_ascii_digit()) {
                    self.position += 1;
                }
            }
            _ => return Err(()),
        }
        if self
            .peek_byte()
            .is_some_and(|byte| matches!(byte, b'.' | b'e' | b'E' | b'+'))
        {
            return Err(());
        }
        let value = &self.bytes[start..self.position];
        if value == b"-0" {
            return Err(());
        }
        Ok(value)
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), ()> {
        if self.peek_byte() == Some(expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> Result<(), ()> {
        if self
            .bytes
            .get(self.position..self.position + expected.len())
            == Some(expected)
        {
            self.position += expected.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }
}

fn hex_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'f' => Some(u32::from(byte - b'a' + 10)),
        _ => None,
    }
}

fn metadata_top_level_kind_is_valid(index: usize, kind: MetadataJsonKind) -> bool {
    match index {
        0 | 11 => kind == MetadataJsonKind::Number,
        1..=7 | 9..=10 | 12..=13 | 15..=17 => kind == MetadataJsonKind::String,
        8 => kind == MetadataJsonKind::Object,
        14 => matches!(kind, MetadataJsonKind::Null | MetadataJsonKind::String),
        18..=21 => kind == MetadataJsonKind::Array,
        _ => false,
    }
}

fn read_metadata_bytes(root: &Path, path: &Path) -> Result<Vec<u8>, StaticInputError> {
    let canonical = fs::canonicalize(path).map_err(|e| StaticInputError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    ensure_inside(root, &canonical)?;
    fs::read(&canonical).map_err(|e| StaticInputError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn ensure_metadata_parent_inside(root: &Path, path: &Path) -> Result<(), StaticInputError> {
    let mut current = path.parent();
    while let Some(candidate) = current {
        match fs::symlink_metadata(candidate) {
            Ok(_) => {
                let canonical = fs::canonicalize(candidate).map_err(|e| StaticInputError::Io {
                    path: candidate.to_path_buf(),
                    message: e.to_string(),
                })?;
                return ensure_inside(root, &canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current = candidate.parent();
            }
            Err(error) => {
                return Err(StaticInputError::Io {
                    path: candidate.to_path_buf(),
                    message: error.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_metadata_entry(
    entry: &CheckedMetadataInput,
    descriptor_id: &str,
) -> Result<(), StaticInputError> {
    let expected = metadata_logical_path(descriptor_id, entry.driver)?;
    if entry.logical_path != expected {
        return Err(StaticInputError::NonCanonical(format!(
            "metadata path `{}` does not equal `{expected}`",
            entry.logical_path
        )));
    }
    if let MetadataState::Present { format_version, .. } = entry.state
        && format_version == 0
    {
        return Err(StaticInputError::NonCanonical(
            "metadata format version is zero".to_string(),
        ));
    }
    Ok(())
}

fn validate_input(input: &StaticInput) -> Result<(), StaticInputError> {
    validate_descriptor_id(&input.descriptor_id)?;
    match &input.source {
        SqlSourceIdentity::File { logical_path } => {
            validate_text_field(logical_path, "file logical path")?;
            if logical_path.is_empty()
                || logical_path.starts_with('/')
                || logical_path.contains('\\')
            {
                return Err(StaticInputError::NonCanonical(
                    "file source is not root-relative".to_string(),
                ));
            }
            if logical_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            {
                return Err(StaticInputError::NonCanonical(
                    "file source is not canonical".to_string(),
                ));
            }
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            validate_text_field(query_or_command_id, "inline descriptor id")?;
            if query_or_command_id != &input.descriptor_id {
                return Err(StaticInputError::NonCanonical(
                    "inline source id mismatch".to_string(),
                ));
            }
        }
    }
    if input.checked_metadata.len() > MAX_SEQUENCE {
        return Err(StaticInputError::NonCanonical(
            "too many checked metadata entries".to_string(),
        ));
    }
    let expected_drivers = input.driver_restriction.drivers();
    if input.checked_metadata.len() != expected_drivers.len() {
        return Err(StaticInputError::NonCanonical(
            "checked metadata does not cover every permitted driver".to_string(),
        ));
    }
    let mut previous_driver = None;
    for (entry, expected_driver) in input.checked_metadata.iter().zip(expected_drivers) {
        validate_text_field(&entry.logical_path, "metadata logical path")?;
        if entry.driver != *expected_driver {
            return Err(StaticInputError::NonCanonical(
                "checked metadata is not in permitted-driver order".to_string(),
            ));
        }
        validate_metadata_entry(entry, &input.descriptor_id)?;
        if let Some(previous) = previous_driver
            && entry.driver <= previous
        {
            return Err(StaticInputError::NonCanonical(
                "checked metadata is not sorted or contains a duplicate driver".to_string(),
            ));
        }
        previous_driver = Some(entry.driver);
    }
    Ok(())
}

fn input_cmp(left: &StaticInput, right: &StaticInput) -> Ordering {
    source_sort_key(&left.source)
        .cmp(&source_sort_key(&right.source))
        .then_with(|| left.consumer_kind.cmp(&right.consumer_kind))
        .then_with(|| {
            left.descriptor_id
                .as_bytes()
                .cmp(right.descriptor_id.as_bytes())
        })
}

fn reject_duplicate_descriptor_ids(inputs: &[StaticInput]) -> Result<(), StaticInputError> {
    let mut descriptor_ids: Vec<&str> = inputs
        .iter()
        .map(|input| input.descriptor_id.as_str())
        .collect();
    descriptor_ids.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    if descriptor_ids
        .windows(2)
        .any(|pair| pair[0].as_bytes() == pair[1].as_bytes())
    {
        return Err(StaticInputError::NonCanonical(
            "duplicate descriptor id".to_string(),
        ));
    }
    Ok(())
}

impl StaticInputManifest {
    pub fn new(
        resolution_digest: Hash128,
        mut inputs: Vec<StaticInput>,
    ) -> Result<Self, StaticInputError> {
        for input in &inputs {
            validate_input(input)?;
        }
        reject_duplicate_descriptor_ids(&inputs)?;
        inputs.sort_by(input_cmp);
        for pair in inputs.windows(2) {
            if input_cmp(&pair[0], &pair[1]) == Ordering::Equal {
                return Err(StaticInputError::NonCanonical(
                    "duplicate static input".to_string(),
                ));
            }
        }
        Ok(Self {
            resolution_digest,
            inputs,
        })
    }

    pub fn empty(resolution_digest: Hash128) -> Self {
        Self {
            resolution_digest,
            inputs: Vec::new(),
        }
    }

    fn validate_canonical(&self) -> Result<(), StaticInputError> {
        if self.inputs.len() > MAX_SEQUENCE {
            return Err(StaticInputError::NonCanonical(
                "too many static inputs".to_string(),
            ));
        }
        for input in &self.inputs {
            validate_input(input)?;
        }
        reject_duplicate_descriptor_ids(&self.inputs)?;
        for pair in self.inputs.windows(2) {
            if input_cmp(&pair[0], &pair[1]) != Ordering::Less {
                return Err(StaticInputError::NonCanonical(
                    "static inputs are not canonically sorted".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StaticInputError> {
        self.validate_canonical()?;
        let mut writer = Writer::new();
        writer.bytes(&STATIC_INPUT_MANIFEST_MAGIC);
        writer.u32(STATIC_INPUT_MANIFEST_FORMAT_VERSION);
        writer.h128(self.resolution_digest);
        writer.seq_len(self.inputs.len())?;
        for input in &self.inputs {
            write_input(&mut writer, input)?;
        }
        Ok(writer.buf)
    }

    pub fn action_key(&self) -> Result<Hash128, StaticInputError> {
        Ok(Hash128::of(&self.canonical_bytes()?))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, StaticInputError> {
        let mut reader = Reader::new(bytes);
        if reader.bytes_exact(STATIC_INPUT_MANIFEST_MAGIC.len())? != STATIC_INPUT_MANIFEST_MAGIC {
            return Err(StaticInputError::BadMagic);
        }
        let version = reader.u32()?;
        if version != STATIC_INPUT_MANIFEST_FORMAT_VERSION {
            return Err(StaticInputError::UnknownVersion(version));
        }
        let resolution_digest = reader.h128()?;
        let inputs = reader.seq(|reader| read_input(reader))?;
        reader.finish()?;
        let manifest = Self {
            resolution_digest,
            inputs,
        };
        manifest.validate_canonical()?;
        Ok(manifest)
    }

    pub fn revalidate(&self, project_root: &Path) -> Result<(), StaticInputError> {
        self.validate_canonical()?;
        let root = canonical_root(project_root)?;
        for input in &self.inputs {
            if let Some(logical) = source_identity_path(&input.source) {
                let candidate = root.join(logical.replace('/', std::path::MAIN_SEPARATOR_STR));
                let bytes = match read_static_bytes(&root, &candidate, logical) {
                    Ok(bytes) => bytes,
                    Err(StaticInputError::MissingFile(_)) => {
                        return Err(StaticInputError::Stale(format!(
                            "file `{logical}` was deleted"
                        )));
                    }
                    Err(StaticInputError::InvalidUtf8 { .. })
                    | Err(StaticInputError::EmbeddedNul { .. }) => {
                        return Err(StaticInputError::Stale(format!(
                            "file `{logical}` is no longer valid text"
                        )));
                    }
                    Err(error) => return Err(error),
                };
                if Hash128::of(&bytes) != input.content_hash {
                    return Err(StaticInputError::HashMismatch {
                        logical_path: logical.to_string(),
                    });
                }
            }
            for metadata in &input.checked_metadata {
                revalidate_metadata(&root, input, metadata)?;
            }
        }
        Ok(())
    }
}

fn revalidate_metadata(
    root: &Path,
    input: &StaticInput,
    expected: &CheckedMetadataInput,
) -> Result<(), StaticInputError> {
    let path = root.join(
        expected
            .logical_path
            .replace('/', std::path::MAIN_SEPARATOR_STR),
    );
    let current = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {
            let bytes = read_metadata_bytes(root, &path)?;
            let format_version = parse_metadata_format_version(&bytes, &expected.logical_path)?;
            Some((Hash128::of(&bytes), format_version))
        }
        Ok(_) => return Err(StaticInputError::NotRegularFile(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure_metadata_parent_inside(root, &path)?;
            None
        }
        Err(error) => {
            return Err(StaticInputError::Io {
                path,
                message: error.to_string(),
            });
        }
    };
    let matches = match (&expected.state, current) {
        (MetadataState::Missing, None) => true,
        (MetadataState::Missing, Some(_)) => false,
        (
            MetadataState::Present {
                content_hash,
                format_version,
            },
            Some((actual_hash, actual_version)),
        ) => *content_hash == actual_hash && *format_version == actual_version,
        (MetadataState::Present { .. }, None) => false,
    };
    if matches {
        Ok(())
    } else {
        Err(StaticInputError::Stale(format!(
            "metadata `{}` for `{}` changed",
            expected.logical_path, input.descriptor_id
        )))
    }
}

/// Compose the existing per-unit MIR identity with the canonical static-input digest. This is the
/// one cache-facing helper: static-input registration does not introduce a parallel Query cache.
pub fn compose_codegen_impl_hash(impl_hash: Hash128, static_inputs_digest: Hash128) -> Hash128 {
    let mut bytes = Vec::with_capacity(8 + 16 + 16);
    bytes.extend_from_slice(b"ALIGNIMP");
    bytes.extend_from_slice(&impl_hash.lo.to_le_bytes());
    bytes.extend_from_slice(&impl_hash.hi.to_le_bytes());
    bytes.extend_from_slice(&static_inputs_digest.lo.to_le_bytes());
    bytes.extend_from_slice(&static_inputs_digest.hi.to_le_bytes());
    Hash128::of(&bytes)
}

fn write_input(writer: &mut Writer, input: &StaticInput) -> Result<(), StaticInputError> {
    writer.str(&input.descriptor_id)?;
    match &input.source {
        SqlSourceIdentity::File { logical_path } => {
            writer.u8(0);
            writer.str(logical_path)?;
        }
        SqlSourceIdentity::Inline {
            query_or_command_id,
        } => {
            writer.u8(1);
            writer.str(query_or_command_id)?;
        }
    }
    writer.h128(input.content_hash);
    writer.u8(input.consumer_kind as u8);
    writer.u8(input.driver_restriction as u8);
    writer.seq_len(input.checked_metadata.len())?;
    for entry in &input.checked_metadata {
        writer.u8(entry.driver as u8);
        writer.str(&entry.logical_path)?;
        match entry.state {
            MetadataState::Missing => writer.u8(0),
            MetadataState::Present {
                content_hash,
                format_version,
            } => {
                writer.u8(1);
                writer.h128(content_hash);
                writer.u32(format_version);
            }
        }
    }
    Ok(())
}

fn read_input(reader: &mut Reader<'_>) -> Result<StaticInput, StaticInputError> {
    let descriptor_id = reader.str()?;
    let source = match reader.u8()? {
        0 => SqlSourceIdentity::File {
            logical_path: reader.str()?,
        },
        1 => SqlSourceIdentity::Inline {
            query_or_command_id: reader.str()?,
        },
        tag => {
            return Err(StaticInputError::BadTag {
                what: "source",
                tag,
            });
        }
    };
    let content_hash = reader.h128()?;
    let consumer_kind = match reader.u8()? {
        0 => StaticConsumerKind::Query,
        1 => StaticConsumerKind::Command,
        tag => {
            return Err(StaticInputError::BadTag {
                what: "consumer kind",
                tag,
            });
        }
    };
    let driver_restriction = match reader.u8()? {
        0 => DriverRestriction::AnySupportedDriver,
        1 => DriverRestriction::SQLiteOnly,
        2 => DriverRestriction::PostgreSQLOnly,
        tag => {
            return Err(StaticInputError::BadTag {
                what: "driver restriction",
                tag,
            });
        }
    };
    let checked_metadata = reader.seq(|reader| {
        let driver = match reader.u8()? {
            0 => Driver::SQLite,
            1 => Driver::PostgreSQL,
            tag => {
                return Err(StaticInputError::BadTag {
                    what: "driver",
                    tag,
                });
            }
        };
        let logical_path = reader.str()?;
        let state = match reader.u8()? {
            0 => MetadataState::Missing,
            1 => MetadataState::Present {
                content_hash: reader.h128()?,
                format_version: reader.u32()?,
            },
            tag => {
                return Err(StaticInputError::BadTag {
                    what: "metadata state",
                    tag,
                });
            }
        };
        Ok(CheckedMetadataInput {
            driver,
            logical_path,
            state,
        })
    })?;
    Ok(StaticInput {
        descriptor_id,
        source,
        content_hash,
        consumer_kind,
        driver_restriction,
        checked_metadata,
    })
}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    fn u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn h128(&mut self, value: Hash128) {
        self.buf.extend_from_slice(&value.lo.to_le_bytes());
        self.buf.extend_from_slice(&value.hi.to_le_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn str(&mut self, value: &str) -> Result<(), StaticInputError> {
        let bytes = value.as_bytes();
        if bytes.len() > MAX_FIELD_BYTES {
            return Err(StaticInputError::NonCanonical(
                "field exceeds the writer limit".to_string(),
            ));
        }
        let length = u32::try_from(bytes.len())
            .map_err(|_| StaticInputError::NonCanonical("field exceeds u32::MAX".to_string()))?;
        self.u32(length);
        self.bytes(bytes);
        Ok(())
    }

    fn seq_len(&mut self, length: usize) -> Result<(), StaticInputError> {
        let length = u32::try_from(length)
            .map_err(|_| StaticInputError::NonCanonical("sequence exceeds u32::MAX".to_string()))?;
        self.u32(length);
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], StaticInputError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(StaticInputError::Truncated)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(StaticInputError::Truncated)?;
        self.offset = end;
        Ok(result)
    }

    fn bytes_exact(&mut self, length: usize) -> Result<&'a [u8], StaticInputError> {
        self.take(length)
    }

    fn u8(&mut self) -> Result<u8, StaticInputError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, StaticInputError> {
        let bytes = self.take(4)?;
        let bytes: [u8; 4] = bytes.try_into().map_err(|_| StaticInputError::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn h128(&mut self) -> Result<Hash128, StaticInputError> {
        let lo = self.u64()?;
        let hi = self.u64()?;
        Ok(Hash128 { lo, hi })
    }

    fn u64(&mut self) -> Result<u64, StaticInputError> {
        let bytes = self.take(8)?;
        let bytes: [u8; 8] = bytes.try_into().map_err(|_| StaticInputError::Truncated)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn str(&mut self) -> Result<String, StaticInputError> {
        let length = usize::try_from(self.u32()?).map_err(|_| StaticInputError::Truncated)?;
        if length > MAX_FIELD_BYTES {
            return Err(StaticInputError::NonCanonical(
                "field exceeds the reader limit".to_string(),
            ));
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| StaticInputError::BadUtf8)
    }

    fn seq<T>(
        &mut self,
        mut read: impl FnMut(&mut Reader<'a>) -> Result<T, StaticInputError>,
    ) -> Result<Vec<T>, StaticInputError> {
        let length = usize::try_from(self.u32()?).map_err(|_| StaticInputError::Truncated)?;
        if length > MAX_SEQUENCE {
            return Err(StaticInputError::NonCanonical(
                "sequence exceeds the reader limit".to_string(),
            ));
        }
        let mut values = Vec::with_capacity(length.min(1024));
        for _ in 0..length {
            values.push(read(self)?);
        }
        Ok(values)
    }

    fn finish(self) -> Result<(), StaticInputError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(StaticInputError::TrailingBytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};
    use std::ops::Deref;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRoot(PathBuf);

    impl Deref for TempRoot {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(label: &str) -> TempRoot {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "align-static-input-{label}-{}-{nonce}",
            std::process::id()
        ));
        create_dir_all(&root).expect("temporary test root");
        TempRoot(root)
    }

    fn metadata(id: &str, driver: Driver, state: MetadataState) -> CheckedMetadataInput {
        CheckedMetadataInput {
            driver,
            logical_path: metadata_logical_path(id, driver).expect("metadata path"),
            state,
        }
    }

    fn metadata_json(format_version: u32, source_sql_hash: &str) -> Vec<u8> {
        let mut bytes = format!(
            "{{\"format_version\":{format_version},\"descriptor_id\":\"q.query\",\"module\":\"q\",\"item\":\"query\",\"driver\":\"sqlite\",\"driver_restriction\":\"sqlite_only\",\"statement_kind\":\"query\",\"statement_class\":\"select\",\"source_identity\":{{\"kind\":\"inline\",\"descriptor_id\":\"q.query\"}},\"source_sql_hash\":\"{source_sql_hash}\",\"wire_sql_hash\":\"00000000000000000000000000000000\",\"rewrite_format_version\":1,\"static_options_hash\":\"00000000000000000000000000000000\",\"params_fingerprint\":\"00000000000000000000000000000000\",\"row_fingerprint\":\"00000000000000000000000000000000\",\"schema_fingerprint\":\"00000000000000000000000000000000\",\"engine_version\":\"sqlite\",\"driver_version\":\"sqlite\",\"search_path\":[],\"extensions\":[],\"parameters\":[],\"columns\":[]}}"
        )
        .into_bytes();
        bytes.push(b'\n');
        bytes
    }

    fn reference_manifest_bytes(manifest: &StaticInputManifest) -> Vec<u8> {
        fn u8(bytes: &mut Vec<u8>, value: u8) {
            bytes.push(value);
        }
        fn u32(bytes: &mut Vec<u8>, value: u32) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn hash(bytes: &mut Vec<u8>, value: Hash128) {
            bytes.extend_from_slice(&value.lo.to_le_bytes());
            bytes.extend_from_slice(&value.hi.to_le_bytes());
        }
        fn string(bytes: &mut Vec<u8>, value: &str) {
            u32(
                bytes,
                u32::try_from(value.len()).expect("reference field length"),
            );
            bytes.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&STATIC_INPUT_MANIFEST_MAGIC);
        u32(&mut bytes, STATIC_INPUT_MANIFEST_FORMAT_VERSION);
        hash(&mut bytes, manifest.resolution_digest);
        u32(
            &mut bytes,
            u32::try_from(manifest.inputs.len()).expect("reference input count"),
        );
        for input in &manifest.inputs {
            string(&mut bytes, &input.descriptor_id);
            match &input.source {
                SqlSourceIdentity::File { logical_path } => {
                    u8(&mut bytes, 0);
                    string(&mut bytes, logical_path);
                }
                SqlSourceIdentity::Inline {
                    query_or_command_id,
                } => {
                    u8(&mut bytes, 1);
                    string(&mut bytes, query_or_command_id);
                }
            }
            hash(&mut bytes, input.content_hash);
            u8(&mut bytes, input.consumer_kind as u8);
            u8(&mut bytes, input.driver_restriction as u8);
            u32(
                &mut bytes,
                u32::try_from(input.checked_metadata.len()).expect("reference metadata count"),
            );
            for entry in &input.checked_metadata {
                u8(&mut bytes, entry.driver as u8);
                string(&mut bytes, &entry.logical_path);
                match entry.state {
                    MetadataState::Missing => u8(&mut bytes, 0),
                    MetadataState::Present {
                        content_hash,
                        format_version,
                    } => {
                        u8(&mut bytes, 1);
                        hash(&mut bytes, content_hash);
                        u32(&mut bytes, format_version);
                    }
                }
            }
        }
        bytes
    }

    #[test]
    fn resolves_sibling_and_registers_root_relative_source() {
        let root = temp_root("sibling");
        let module = root.join("queries/user.align");
        create_dir_all(module.parent().expect("module parent")).expect("query directory");
        write(&module, "module queries.user\n").expect("align source");
        write(module.with_extension("sql"), "select $id\n").expect("sql source");
        let mut source_map = SourceMap::new();
        let resolved = resolve_static_file(
            &root,
            &module,
            None,
            "queries.user.query",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
            Some(&mut source_map),
        )
        .expect("sibling SQL");
        assert_eq!(
            resolved.input.source,
            SqlSourceIdentity::File {
                logical_path: "queries/user.sql".into()
            }
        );
        assert_eq!(resolved.input.content_hash, Hash128::of(b"select $id\n"));
        let file_id = resolved.source_map_file.expect("SourceMap registration");
        assert_eq!(source_map.get(file_id).name, "queries/user.sql");
        assert_eq!(source_map.get(file_id).src, "select $id\n");
    }

    #[test]
    fn explicit_path_rejects_root_escape_and_symlink_escape() {
        let root = temp_root("escape");
        let module = root.join("queries/user.align");
        create_dir_all(module.parent().expect("module parent")).expect("query directory");
        write(&module, "module queries.user\n").expect("align source");
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("../secret.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        let outside_root = temp_root("outside");
        let outside = outside_root.join("secret.sql");
        write(&outside, "secret").expect("outside SQL");
        let link = root.join("queries/link.sql");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        #[cfg(unix)]
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("link.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("/tmp/absolute.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("bad\0.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("bad\\name.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidPath(_))
        ));
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("missing.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::MissingFile(_))
        ));
        create_dir_all(root.join("queries/directory.sql")).expect("directory input");
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                Some("directory.sql"),
                "q",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::NotRegularFile(_))
        ));
    }

    #[test]
    fn invalid_text_reports_utf8_and_first_nul() {
        let root = temp_root("text");
        let module = root.join("q.align");
        write(&module, "module q\n").expect("align source");
        write(root.join("q.sql"), [0xff, 0xfe]).expect("invalid bytes");
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                None,
                "q.query",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::InvalidUtf8 { .. })
        ));
        write(root.join("q.sql"), b"ok\0bad").expect("NUL bytes");
        assert_eq!(
            resolve_static_file(
                &root,
                &module,
                None,
                "q.query",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                None
            ),
            Err(StaticInputError::EmbeddedNul {
                logical_path: "q.sql".into(),
                offset: 2
            })
        );

        write(root.join("q.sql"), "select 1").expect("valid SQL bytes");
        let mut source_map = SourceMap::new();
        assert!(matches!(
            resolve_static_file(
                &root,
                &module,
                None,
                "bad\0descriptor",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
                Some(&mut source_map),
            ),
            Err(StaticInputError::InvalidDescriptorId)
        ));
        assert!(source_map.files().is_empty());
    }

    #[test]
    fn inline_does_not_resolve_a_file_and_identity_is_descriptor_bound() {
        let resolved = resolve_inline_static_input(
            "q.query",
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
        )
        .expect("inline source");
        assert_eq!(resolved.resolved_path, None);
        assert_eq!(resolved.source_map_file, None);
        assert_eq!(
            resolved.input.source,
            SqlSourceIdentity::Inline {
                query_or_command_id: "q.query".into()
            }
        );
        assert_eq!(resolved.input.content_hash, Hash128::of(b"select 1"));
        assert!(matches!(
            resolve_inline_static_input(
                "q.query",
                "bad\0sql",
                StaticConsumerKind::Query,
                DriverRestriction::AnySupportedDriver,
            ),
            Err(StaticInputError::EmbeddedNul { offset: 3, .. })
        ));
    }

    #[test]
    fn metadata_snapshot_and_revalidation_track_missing_present_and_change() {
        let root = temp_root("metadata");
        let id = "q.query";
        let input = resolve_inline_static_input(
            id,
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("inline input")
        .input;
        let missing = snapshot_checked_metadata(&root, id, Driver::SQLite).expect("missing state");
        assert_eq!(missing.state, MetadataState::Missing);
        let manifest = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![missing],
                ..input.clone()
            }],
        )
        .expect("missing manifest");
        manifest.revalidate(&root).expect("missing remains valid");
        let path = metadata_path(&root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(path.parent().expect("metadata directory")).expect("metadata directory");
        write(&path, metadata_json(1, "00000000000000000000000000000000")).expect("metadata");
        assert!(matches!(
            manifest.revalidate(&root),
            Err(StaticInputError::Stale(_))
        ));
        let present = snapshot_checked_metadata(&root, id, Driver::SQLite).expect("present state");
        let present_manifest = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![present],
                ..input
            }],
        )
        .expect("present manifest");
        present_manifest
            .revalidate(&root)
            .expect("present remains valid");
        write(&path, metadata_json(2, "00000000000000000000000000000000")).expect("metadata edit");
        assert!(matches!(
            present_manifest.revalidate(&root),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        write(&path, metadata_json(1, "10000000000000000000000000000000"))
            .expect("changed metadata");
        assert!(matches!(
            present_manifest.revalidate(&root),
            Err(StaticInputError::Stale(_))
        ));
    }

    #[test]
    fn metadata_parser_consumes_complete_canonical_v1_record() {
        let root = temp_root("metadata-malformed");
        let id = "q.query";
        let path = metadata_path(&root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(path.parent().expect("metadata directory")).expect("metadata directory");

        let mut malformed = metadata_json(1, "00000000000000000000000000000000");
        malformed.extend_from_slice(b"garbage");
        write(&path, malformed).expect("malformed metadata");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));

        write(&path, metadata_json(2, "00000000000000000000000000000000")).expect("v2 metadata");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::MetadataMalformed { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn metadata_parent_symlink_cannot_escape_project_root() {
        let root = temp_root("metadata-link");
        let outside_root = temp_root("metadata-link-outside");
        let id = "q.query";
        let outside_path = metadata_path(&outside_root, id, Driver::SQLite).expect("metadata path");
        create_dir_all(outside_path.parent().expect("metadata directory"))
            .expect("metadata directory");
        write(
            &outside_path,
            metadata_json(1, "00000000000000000000000000000000"),
        )
        .expect("outside metadata");
        std::os::unix::fs::symlink(outside_root.join(".align-db"), root.join(".align-db"))
            .expect("metadata directory symlink");
        assert!(matches!(
            snapshot_checked_metadata(&root, id, Driver::SQLite),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));

        let missing_root = temp_root("metadata-link-missing");
        let missing_outside = temp_root("metadata-link-missing-outside");
        create_dir_all(missing_outside.join(".align-db")).expect("outside metadata directory");
        std::os::unix::fs::symlink(
            missing_outside.join(".align-db"),
            missing_root.join(".align-db"),
        )
        .expect("missing metadata directory symlink");
        assert!(matches!(
            snapshot_checked_metadata(&missing_root, id, Driver::SQLite),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));
        let input = resolve_inline_static_input(
            id,
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("missing metadata input")
        .input;
        let manifest = StaticInputManifest::new(
            Hash128 { lo: 3, hi: 4 },
            vec![StaticInput {
                checked_metadata: vec![metadata(id, Driver::SQLite, MetadataState::Missing)],
                ..input
            }],
        )
        .expect("missing metadata manifest");
        assert!(matches!(
            manifest.revalidate(&missing_root),
            Err(StaticInputError::OutsideProjectRoot(_))
        ));
    }

    #[test]
    fn manifest_codec_is_canonical_and_fail_closed() {
        let mut input = resolve_inline_static_input(
            "z.command",
            "delete",
            StaticConsumerKind::Command,
            DriverRestriction::SQLiteOnly,
        )
        .expect("command input")
        .input;
        input.checked_metadata = vec![metadata(
            "z.command",
            Driver::SQLite,
            MetadataState::Missing,
        )];
        let mut other = resolve_inline_static_input(
            "a.query",
            "select",
            StaticConsumerKind::Query,
            DriverRestriction::PostgreSQLOnly,
        )
        .expect("query input")
        .input;
        other.checked_metadata = vec![metadata(
            "a.query",
            Driver::PostgreSQL,
            MetadataState::Missing,
        )];
        let manifest = StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![input, other])
            .expect("manifest");
        let bytes = manifest.canonical_bytes().expect("canonical bytes");
        let reference = reference_manifest_bytes(&manifest);
        assert_eq!(bytes, reference);
        assert_eq!(
            StaticInputManifest::decode(&bytes).expect("decode"),
            manifest
        );
        assert_eq!(
            StaticInputManifest::decode(&reference).expect("reference decode"),
            manifest
        );
        assert_eq!(manifest.action_key().expect("digest"), Hash128::of(&bytes));
        let mut bad_magic = bytes.clone();
        bad_magic[0] ^= 1;
        assert_eq!(
            StaticInputManifest::decode(&bad_magic),
            Err(StaticInputError::BadMagic)
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            StaticInputManifest::decode(&trailing),
            Err(StaticInputError::TrailingBytes)
        );
        let truncated = &bytes[..bytes.len() - 1];
        assert_eq!(
            StaticInputManifest::decode(truncated),
            Err(StaticInputError::Truncated)
        );
        let mut huge_count = bytes.clone();
        huge_count[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            StaticInputManifest::decode(&huge_count),
            Err(StaticInputError::NonCanonical(_))
        ));
        let descriptor_len =
            u32::from_le_bytes(bytes[32..36].try_into().expect("descriptor length bytes"));
        let source_tag = 36 + usize::try_from(descriptor_len).expect("descriptor length");
        let mut bad_tag = bytes.clone();
        bad_tag[source_tag] = 9;
        assert!(matches!(
            StaticInputManifest::decode(&bad_tag),
            Err(StaticInputError::BadTag {
                what: "source",
                tag: 9
            })
        ));
        let mut bad_utf8 = bytes.clone();
        bad_utf8[source_tag + 5] = 0xff;
        assert_eq!(
            StaticInputManifest::decode(&bad_utf8),
            Err(StaticInputError::BadUtf8)
        );
        let mut duplicate = manifest.clone();
        duplicate.inputs = vec![manifest.inputs[0].clone(), manifest.inputs[0].clone()];
        assert!(matches!(
            StaticInputManifest::decode(&reference_manifest_bytes(&duplicate)),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut bad_metadata = manifest
            .inputs
            .iter()
            .find(|input| !input.checked_metadata.is_empty())
            .expect("metadata-bearing input")
            .clone();
        bad_metadata.checked_metadata[0].logical_path = ".align-db/sqlite/wrong.json".into();
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![bad_metadata]),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut omitted_metadata = manifest
            .inputs
            .iter()
            .find(|input| !input.checked_metadata.is_empty())
            .expect("metadata-bearing input")
            .clone();
        omitted_metadata.checked_metadata.clear();
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![omitted_metadata]),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut wrong_order = resolve_inline_static_input(
            "any.query",
            "select",
            StaticConsumerKind::Query,
            DriverRestriction::AnySupportedDriver,
        )
        .expect("any-driver input")
        .input;
        wrong_order.checked_metadata = vec![
            metadata("any.query", Driver::PostgreSQL, MetadataState::Missing),
            metadata("any.query", Driver::SQLite, MetadataState::Missing),
        ];
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 7, hi: 8 }, vec![wrong_order]),
            Err(StaticInputError::NonCanonical(_))
        ));
        let mut unsorted = manifest.clone();
        unsorted.inputs.swap(0, 1);
        assert!(matches!(
            unsorted.canonical_bytes(),
            Err(StaticInputError::NonCanonical(_))
        ));
    }

    #[test]
    fn manifest_rejects_nul_file_identity_and_independent_descriptor_duplicates() {
        let mut nul = resolve_inline_static_input(
            "nul.query",
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("nul test input")
        .input;
        nul.source = SqlSourceIdentity::File {
            logical_path: "queries/bad\0.sql".to_string(),
        };
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 9, hi: 10 }, vec![nul]),
            Err(StaticInputError::NonCanonical(_))
        ));

        let first = resolve_inline_static_input(
            "duplicate.query",
            "select 1",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
        )
        .expect("first duplicate input")
        .input;
        let mut second = first.clone();
        second.source = SqlSourceIdentity::File {
            logical_path: "queries/other.sql".to_string(),
        };
        second.consumer_kind = StaticConsumerKind::Command;
        assert!(matches!(
            StaticInputManifest::new(Hash128 { lo: 9, hi: 10 }, vec![first, second]),
            Err(StaticInputError::NonCanonical(_))
        ));
    }

    #[test]
    fn metadata_paths_are_exact_and_checkout_root_independent() {
        let id = "queries.user";
        let sqlite = metadata_logical_path(id, Driver::SQLite).expect("sqlite path");
        let postgres = metadata_logical_path(id, Driver::PostgreSQL).expect("postgres path");
        assert_eq!(
            sqlite,
            ".align-db/sqlite/".to_string() + &Hash128::of(id.as_bytes()).to_hex() + ".json"
        );
        assert_eq!(
            postgres,
            ".align-db/postgres/".to_string() + &Hash128::of(id.as_bytes()).to_hex() + ".json"
        );
    }

    #[test]
    fn file_deletion_is_a_manifest_stale_result() {
        let root = temp_root("deletion");
        let module = root.join("q.align");
        write(&module, "module q\n").expect("align source");
        let sql = root.join("q.sql");
        write(&sql, "select 1").expect("SQL source");
        let input = resolve_static_file(
            &root,
            &module,
            None,
            "q.query",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("file input")
        .input;
        let manifest = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![metadata("q.query", Driver::SQLite, MetadataState::Missing)],
                ..input
            }],
        )
        .expect("manifest");
        std::fs::remove_file(sql).expect("delete SQL source");
        assert!(matches!(
            manifest.revalidate(&root),
            Err(StaticInputError::Stale(_))
        ));
    }

    #[test]
    fn codegen_identity_includes_static_inputs_without_path_or_mtime() {
        let base = Hash128 { lo: 10, hi: 11 };
        let first = StaticInputManifest::empty(Hash128 { lo: 1, hi: 2 })
            .action_key()
            .expect("digest");
        let second = StaticInputManifest::empty(Hash128 { lo: 3, hi: 4 })
            .action_key()
            .expect("digest");
        assert_ne!(
            compose_codegen_impl_hash(base, first),
            compose_codegen_impl_hash(base, second)
        );
        assert_eq!(
            compose_codegen_impl_hash(base, first),
            compose_codegen_impl_hash(base, first)
        );
    }

    #[test]
    fn equivalent_checkout_roots_have_identical_manifest_identity() {
        let first_root = temp_root("checkout-a");
        let second_root = temp_root("checkout-b");
        for root in [&first_root, &second_root] {
            let module = root.join("q.align");
            write(&module, "module q\n").expect("align source");
            write(root.join("q.sql"), "select 1").expect("SQL source");
        }
        let first = resolve_static_file(
            &first_root,
            &first_root.join("q.align"),
            None,
            "q.query",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("first checkout")
        .input;
        let second = resolve_static_file(
            &second_root,
            &second_root.join("q.align"),
            None,
            "q.query",
            StaticConsumerKind::Query,
            DriverRestriction::SQLiteOnly,
            None,
        )
        .expect("second checkout")
        .input;
        let first = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![metadata("q.query", Driver::SQLite, MetadataState::Missing)],
                ..first
            }],
        )
        .expect("first manifest");
        let second = StaticInputManifest::new(
            Hash128 { lo: 1, hi: 2 },
            vec![StaticInput {
                checked_metadata: vec![metadata("q.query", Driver::SQLite, MetadataState::Missing)],
                ..second
            }],
        )
        .expect("second manifest");
        assert_eq!(
            first.canonical_bytes().expect("first bytes"),
            second.canonical_bytes().expect("second bytes")
        );
        assert_eq!(
            first.action_key().expect("first digest"),
            second.action_key().expect("second digest")
        );
    }
}
