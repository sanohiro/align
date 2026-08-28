//! Producer-owned input observations for foreground builds.
//!
//! Native file events are only latency hints.  This module owns the semantic
//! state and topology snapshots used both after a build and by periodic watch
//! audits, so the compiler never grows a second path-discovery algorithm.

use align_interface::{Hash128, Hash128Stream};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

const MAX_PATH_BYTES: usize = 1_023;
const MAX_INPUTS: usize = 16_384;
const MAX_MERGED_INPUTS: usize = 32_768;
const MAX_GRAPH_NODES: usize = 65_536;
const MAX_EVIDENCE_NODES: usize = 131_072;
const MAX_SYMLINKS: usize = 40;
const HASH_BUFFER: usize = 64 * 1_024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BuildInputState {
    Missing,
    Regular { content_hash: Hash128, len: u64 },
    NonRegular,
    Unreadable,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BuildInput {
    path: PathBuf,
    state: BuildInputState,
}

impl BuildInput {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn state(&self) -> BuildInputState {
        self.state
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum NodeState {
    Missing,
    Directory {
        device: u64,
        inode: u64,
    },
    Regular {
        device: u64,
        inode: u64,
    },
    Symlink {
        device: u64,
        inode: u64,
        target: OsString,
    },
    Other {
        device: u64,
        inode: u64,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GraphNode {
    path: PathBuf,
    state: NodeState,
}

#[derive(Clone, PartialEq, Eq)]
struct PathGraph {
    root: GraphNode,
    nodes: Vec<GraphNode>,
}

#[derive(Clone, PartialEq, Eq)]
struct ReadEvidence {
    before: PathGraph,
    opened_identity: Option<(u64, u64)>,
    after: PathGraph,
}

#[derive(Clone)]
struct ObservedRow {
    input: BuildInput,
    ordinal: usize,
    evidence: Vec<ReadEvidence>,
}

pub struct BuildInputSet {
    rows: Vec<BuildInput>,
    evidence: BTreeMap<Vec<u8>, Vec<ReadEvidence>>,
    encounter: Vec<Vec<u8>>,
    changed_during_attempt: bool,
}

impl BuildInputSet {
    pub fn inputs(&self) -> &[BuildInput] {
        &self.rows
    }

    pub fn changed_during_attempt(&self) -> bool {
        self.changed_during_attempt
    }
}

pub struct FinalBuildInputSet {
    rows: Vec<BuildInput>,
    graphs: Vec<PathGraph>,
    changed_during_attempt: bool,
}

impl FinalBuildInputSet {
    pub fn inputs(&self) -> &[BuildInput] {
        &self.rows
    }

    pub fn changed_during_attempt(&self) -> bool {
        self.changed_during_attempt
    }
}

#[derive(PartialEq, Eq)]
pub struct WatchRepairDependency {
    path: PathBuf,
    graph: PathGraph,
}

impl WatchRepairDependency {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct FinalizedWatchInputs {
    inputs: FinalBuildInputSet,
    alias_index: Option<usize>,
    repair_dependency: Option<WatchRepairDependency>,
}

pub struct MonitorBaseline {
    rows: Vec<MonitorRow>,
}

struct MonitorRow {
    input: BuildInput,
    graph: PathGraph,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MonitorRefresh {
    Stable,
    Changed,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WatchRegistration {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl WatchRegistration {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn monitor_baseline(
    inputs: &FinalBuildInputSet,
    retained: &[PathBuf],
) -> Result<MonitorBaseline, BuildInputTopologyError> {
    let mut rows = inputs
        .rows
        .iter()
        .cloned()
        .zip(inputs.graphs.iter().cloned())
        .map(|(input, graph)| MonitorRow { input, graph })
        .collect::<Vec<_>>();
    for path in retained {
        if rows.iter().any(|row| row.input.path == *path) {
            continue;
        }
        rows.push(MonitorRow {
            input: BuildInput {
                path: path.clone(),
                state: classify(path).0,
            },
            graph: snapshot_graph(path)?,
        });
    }
    rows.sort_by(|left, right| path_bytes(&left.input.path).cmp(path_bytes(&right.input.path)));
    enforce_graph_limit(rows.iter().map(|row| (&row.input.path, &row.graph)))?;
    Ok(MonitorBaseline { rows })
}

pub fn refresh_monitor_baseline(
    baseline: &mut MonitorBaseline,
) -> Result<MonitorRefresh, BuildInputTopologyError> {
    let mut refreshed = Vec::with_capacity(baseline.rows.len());
    for row in &baseline.rows {
        let mut stable_sample = None;
        for _ in 0..8 {
            let before = snapshot_graph(&row.input.path)?;
            let (state, opened_identity) = classify(&row.input.path);
            let graph = snapshot_graph(&row.input.path)?;
            if opened_identity.is_some_and(|identity| !graph_has_identity(&graph, identity)) {
                continue;
            }
            stable_sample = Some((before, state, opened_identity, graph));
            break;
        }
        let Some((before, state, opened_identity, graph)) = stable_sample else {
            return Ok(MonitorRefresh::Changed);
        };
        if monitor_row_changed(row, &before, state, opened_identity, &graph) {
            return Ok(MonitorRefresh::Changed);
        }
        refreshed.push(MonitorRow {
            input: BuildInput {
                path: row.input.path.clone(),
                state,
            },
            graph,
        });
    }
    baseline.rows = refreshed;
    Ok(MonitorRefresh::Stable)
}

fn monitor_row_changed(
    row: &MonitorRow,
    before: &PathGraph,
    state: BuildInputState,
    opened_identity: Option<(u64, u64)>,
    graph: &PathGraph,
) -> bool {
    let opened_belongs_to_graph =
        opened_identity.is_none_or(|identity| graph_has_identity(graph, identity));
    let regular_rearm = only_regular_leaf_identity_changed(before, graph, state)
        && opened_belongs_to_graph;
    if (before != graph || !opened_belongs_to_graph) && !regular_rearm {
        return true;
    }
    state != row.input.state
        || (graph != &row.graph
            && !only_regular_leaf_identity_changed(&row.graph, graph, state))
}

pub fn monitor_watch_registrations(
    baseline: &MonitorBaseline,
    repair: Option<&WatchRepairDependency>,
) -> Result<Vec<WatchRegistration>, BuildInputTopologyError> {
    enforce_graph_limit(
        baseline
            .rows
            .iter()
            .map(|row| (&row.input.path, &row.graph))
            .chain(
                repair
                    .into_iter()
                    .map(|repair| (&repair.path, &repair.graph)),
            ),
    )?;
    let mut registrations = std::collections::BTreeSet::new();
    for row in &baseline.rows {
        add_graph_watch_registrations(&mut registrations, &row.graph);
    }
    if let Some(repair) = repair {
        add_graph_watch_registrations(&mut registrations, &repair.graph);
    }
    Ok(registrations.into_iter().collect())
}

fn add_graph_watch_registrations(
    registrations: &mut std::collections::BTreeSet<WatchRegistration>,
    graph: &PathGraph,
) {
    let mut added = false;
    for node in &graph.nodes {
        if let NodeState::Directory { device, inode } = node.state {
            registrations.insert(WatchRegistration {
                path: node.path.clone(),
                device,
                inode,
            });
            added = true;
        }
    }
    if !added && let NodeState::Directory { device, inode } = graph.root.state {
        registrations.insert(WatchRegistration {
            path: graph.root.path.clone(),
            device,
            inode,
        });
    }
}

fn only_regular_leaf_identity_changed(
    before: &PathGraph,
    after: &PathGraph,
    state: BuildInputState,
) -> bool {
    if !matches!(state, BuildInputState::Regular { .. })
        || before.root != after.root
        || before.nodes.len() != after.nodes.len()
    {
        return false;
    }
    let mut changed = None;
    for (index, (left, right)) in before.nodes.iter().zip(&after.nodes).enumerate() {
        if left == right {
            continue;
        }
        if changed.replace(index).is_some()
            || left.path != right.path
            || !matches!(left.state, NodeState::Regular { .. })
            || !matches!(right.state, NodeState::Regular { .. })
        {
            return false;
        }
    }
    changed == before.nodes.len().checked_sub(1)
}

impl FinalizedWatchInputs {
    pub fn inputs(&self) -> &FinalBuildInputSet {
        &self.inputs
    }

    pub fn alias_index(&self) -> Option<usize> {
        self.alias_index
    }

    pub fn repair_dependency(&self) -> Option<&WatchRepairDependency> {
        self.repair_dependency.as_ref()
    }

    pub fn into_parts(self) -> (FinalBuildInputSet, Option<WatchRepairDependency>) {
        (self.inputs, self.repair_dependency)
    }
}

pub struct BuildInputTopologyError {
    message: String,
}

impl BuildInputTopologyError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Debug for BuildInputTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("BuildInputTopologyError")
            .field(&self.message)
            .finish()
    }
}

impl fmt::Display for BuildInputTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BuildInputTopologyError {}

pub enum BuildSourceError {
    Missing,
    NonRegular,
    InvalidUtf8 { offset: u64 },
    Io { message: String },
}

struct ObservationCollector {
    rows: BTreeMap<Vec<u8>, ObservedRow>,
    next_ordinal: usize,
    evidence_nodes: usize,
    changed: bool,
    error: Option<BuildInputTopologyError>,
}

impl ObservationCollector {
    fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            next_ordinal: 0,
            evidence_nodes: 0,
            changed: false,
            error: None,
        }
    }

    #[cfg(test)]
    fn observe_path(&mut self, path: &Path) -> Result<BuildInputState, BuildInputTopologyError> {
        validate_absolute_path(path)?;
        let before = snapshot_graph(path)?;
        let (state, opened_identity) = classify(path);
        let after = snapshot_graph(path)?;
        let evidence = ReadEvidence {
            before,
            opened_identity,
            after,
        };
        self.record(path, state, evidence)?;
        Ok(state)
    }

    fn record(
        &mut self,
        path: &Path,
        state: BuildInputState,
        evidence: ReadEvidence,
    ) -> Result<(), BuildInputTopologyError> {
        let key = path_bytes(path).to_vec();
        let ordinal = self.next_ordinal;
        self.next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .ok_or_else(|| BuildInputTopologyError::new("input encounter ordinal exhausted"))?;
        if let Some(existing) = self.rows.get_mut(&key) {
            let new_evidence = !existing.evidence.contains(&evidence);
            if existing.input.state != state || new_evidence {
                self.changed = true;
            }
            existing.input.state = state;
            existing.ordinal = ordinal;
            if new_evidence {
                self.evidence_nodes = self
                    .evidence_nodes
                    .checked_add(evidence_node_count(&evidence))
                    .ok_or_else(|| too_many_evidence(path))?;
                if self.evidence_nodes > MAX_EVIDENCE_NODES {
                    return Err(too_many_evidence(path));
                }
                existing.evidence.push(evidence);
            }
            return Ok(());
        }
        if self.rows.len() == MAX_INPUTS {
            return Err(BuildInputTopologyError::new(format!(
                "too many inputs (maximum {MAX_INPUTS}; next '{}')",
                encode_watch_path(path)
            )));
        }
        let new_evidence_nodes = self
            .evidence_nodes
            .checked_add(evidence_node_count(&evidence))
            .ok_or_else(|| too_many_evidence(path))?;
        if new_evidence_nodes > MAX_EVIDENCE_NODES {
            return Err(too_many_evidence(path));
        }
        self.rows.insert(
            key,
            ObservedRow {
                input: BuildInput {
                    path: path.to_path_buf(),
                    state,
                },
                ordinal,
                evidence: vec![evidence],
            },
        );
        self.evidence_nodes = new_evidence_nodes;
        Ok(())
    }

    fn finish(self) -> Result<BuildInputSet, BuildInputTopologyError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut encounter = self
            .rows
            .iter()
            .map(|(key, row)| (row.ordinal, key.clone()))
            .collect::<Vec<_>>();
        encounter.sort_by_key(|(ordinal, _)| *ordinal);
        let mut evidence = BTreeMap::new();
        let mut rows = Vec::with_capacity(self.rows.len());
        for (key, row) in self.rows {
            evidence.insert(key, row.evidence);
            rows.push(row.input);
        }
        Ok(BuildInputSet {
            rows,
            evidence,
            encounter: encounter.into_iter().map(|(_, key)| key).collect(),
            changed_during_attempt: self.changed,
        })
    }
}

thread_local! {
    static ACTIVE_OBSERVER: RefCell<Option<ObservationCollector>> = const { RefCell::new(None) };
}

struct ObservationScope;

impl Drop for ObservationScope {
    fn drop(&mut self) {
        ACTIVE_OBSERVER.with(|slot| {
            let _ = slot.borrow_mut().take();
        });
    }
}

pub fn collect_observations<T>(
    operation: impl FnOnce() -> T,
) -> (T, Result<BuildInputSet, BuildInputTopologyError>) {
    ACTIVE_OBSERVER.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "build input observations must not nest"
        );
        *slot.borrow_mut() = Some(ObservationCollector::new());
    });
    let scope = ObservationScope;
    let value = operation();
    let collector = ACTIVE_OBSERVER
        .with(|slot| slot.borrow_mut().take())
        .expect("observer installed");
    std::mem::forget(scope);
    (value, collector.finish())
}

pub fn observe_consumed_read<T>(
    path: &Path,
    read: impl FnOnce(io::Result<File>) -> T,
    consumed_bytes: impl FnOnce(&T) -> Option<&[u8]>,
    rejected: impl FnOnce() -> T,
) -> T {
    ACTIVE_OBSERVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(observer) = slot.as_mut() else {
            return read(File::open(path));
        };
        let absolute = match absolute_lexical(path) {
            Ok(path) => path,
            Err(error) => {
                observer.error.get_or_insert(error);
                return rejected();
            }
        };
        let before = match snapshot_graph(&absolute) {
            Ok(before) => before,
            Err(error) => {
                observer.error.get_or_insert(error);
                return rejected();
            }
        };
        let (opened, opened_identity, opened_state) = match open_observed(&absolute) {
            Ok(file) => match file.metadata() {
                Ok(metadata) if metadata.is_file() => {
                    let identity = identity(&metadata);
                    (Ok(file), Some(identity), None)
                }
                Ok(metadata) => (
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not a regular file",
                    )),
                    Some(identity(&metadata)),
                    Some(BuildInputState::NonRegular),
                ),
                Err(error) => (Err(error), None, Some(BuildInputState::Unreadable)),
            },
            Err(error) => {
                let state = if error.kind() == io::ErrorKind::NotFound {
                    BuildInputState::Missing
                } else {
                    BuildInputState::Unreadable
                };
                (Err(error), None, Some(state))
            }
        };
        let value = read(opened);
        let observed = consumed_bytes(&value);
        let state = match observed {
            Some(bytes) => state_from_bytes(bytes),
            None => opened_state.unwrap_or_else(|| classify(&absolute).0),
        };
        let after = snapshot_graph(&absolute);
        match after {
            Ok(after) => {
                if let Err(error) = observer.record(
                    &absolute,
                    state,
                    ReadEvidence {
                        before,
                        opened_identity,
                        after,
                    },
                ) {
                    observer.error.get_or_insert(error);
                }
            }
            Err(error) => {
                observer.error.get_or_insert(error);
            }
        }
        value
    })
}

pub fn observe_consumed_classification<T>(
    path: &Path,
    operation: impl FnOnce() -> T,
    classification: impl FnOnce(&T) -> Option<BuildInputState>,
    observed_metadata: impl FnOnce(&T) -> Option<&fs::Metadata>,
    rejected: impl FnOnce() -> T,
) -> T {
    ACTIVE_OBSERVER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(observer) = slot.as_mut() else {
            return operation();
        };
        let absolute = match absolute_lexical(path) {
            Ok(path) => path,
            Err(error) => {
                observer.error.get_or_insert(error);
                return rejected();
            }
        };
        let before = match snapshot_graph(&absolute) {
            Ok(before) => before,
            Err(error) => {
                observer.error.get_or_insert(error);
                return rejected();
            }
        };
        let value = operation();
        let state = classification(&value);
        let opened_identity = observed_metadata(&value).map(identity);
        let after = snapshot_graph(&absolute);
        if let Some(state) = state {
            match after {
                Ok(after) => {
                    if let Err(error) = observer.record(
                        &absolute,
                        state,
                        ReadEvidence {
                            before,
                            opened_identity,
                            after,
                        },
                    ) {
                        observer.error.get_or_insert(error);
                    }
                }
                Err(error) => {
                    observer.error.get_or_insert(error);
                }
            }
        }
        value
    })
}

fn state_from_bytes(bytes: &[u8]) -> BuildInputState {
    let mut stream = Hash128Stream::for_len(bytes.len());
    if !stream.update(bytes) {
        return BuildInputState::Unreadable;
    }
    match stream.finish() {
        Some(content_hash) => BuildInputState::Regular {
            content_hash,
            len: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        },
        None => BuildInputState::Unreadable,
    }
}

pub fn absolute_lexical(path: &Path) -> Result<PathBuf, BuildInputTopologyError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                BuildInputTopologyError::new(format!("cannot resolve current directory: {error}"))
            })?
            .join(path)
    };
    validate_absolute_path(&absolute)?;
    Ok(absolute)
}

pub fn merge_observed_build_inputs(
    first: BuildInputSet,
    retry: BuildInputSet,
) -> Result<BuildInputSet, BuildInputTopologyError> {
    let BuildInputSet {
        rows: first_rows,
        mut evidence,
        mut encounter,
        changed_during_attempt: first_changed,
    } = first;
    let BuildInputSet {
        rows: retry_rows,
        evidence: mut retry_evidence,
        encounter: retry_encounter,
        changed_during_attempt: retry_changed,
    } = retry;
    let mut rows: BTreeMap<Vec<u8>, BuildInput> = first_rows
        .into_iter()
        .map(|row| (path_bytes(&row.path).to_vec(), row))
        .collect();
    let mut retry_rows = retry_rows
        .into_iter()
        .map(|row| (path_bytes(&row.path).to_vec(), row))
        .collect::<BTreeMap<_, _>>();
    let mut changed = first_changed || retry_changed;
    let mut evidence_nodes = evidence
        .values()
        .flatten()
        .map(evidence_node_count)
        .sum::<usize>();
    for key in retry_encounter {
        let Some(row) = retry_rows.remove(&key) else {
            continue;
        };
        if !rows.contains_key(&key) && rows.len() == MAX_MERGED_INPUTS {
            return Err(BuildInputTopologyError::new(format!(
                "too many merged inputs (maximum {MAX_MERGED_INPUTS}; next '{}')",
                encode_watch_path(row.path())
            )));
        }
        if let Some(previous) = rows.insert(key.clone(), row.clone())
            && previous.state != row.state
        {
            changed = true;
        }
        let additions = retry_evidence.remove(&key).unwrap_or_default();
        let overlap = evidence.contains_key(&key);
        let retained = evidence.entry(key.clone()).or_default();
        for read in additions {
            if retained.contains(&read) {
                continue;
            }
            if overlap {
                changed = true;
            }
            evidence_nodes = evidence_nodes
                .checked_add(evidence_node_count(&read))
                .ok_or_else(|| too_many_evidence(row.path()))?;
            if evidence_nodes > MAX_EVIDENCE_NODES {
                return Err(too_many_evidence(row.path()));
            }
            retained.push(read);
        }
        encounter.push(key);
    }
    let rows = rows.into_values().collect();
    Ok(BuildInputSet {
        rows,
        evidence,
        encounter,
        changed_during_attempt: changed,
    })
}

fn evidence_node_count(read: &ReadEvidence) -> usize {
    read.before.nodes.len() + read.after.nodes.len()
}

pub fn snapshot_watch_repair(
    path: &Path,
) -> Result<WatchRepairDependency, BuildInputTopologyError> {
    validate_absolute_path(path)?;
    Ok(WatchRepairDependency {
        path: path.to_path_buf(),
        graph: snapshot_graph(path)?,
    })
}

pub fn finalize_watch_inputs(
    inputs: BuildInputSet,
    output: Option<&Path>,
) -> Result<FinalizedWatchInputs, BuildInputTopologyError> {
    if let Some(path) = output {
        validate_absolute_path(path)?;
    }
    let mut final_rows = Vec::with_capacity(inputs.rows.len());
    let mut changed = inputs.changed_during_attempt;
    let mut final_graphs = Vec::with_capacity(inputs.rows.len());
    for original in &inputs.rows {
        let before = snapshot_graph(&original.path)?;
        let (state, opened_identity) = classify(&original.path);
        let graph = snapshot_graph(&original.path)?;
        if state != original.state {
            changed = true;
        }
        if before != graph
            || opened_identity.is_some_and(|identity| !graph_has_identity(&graph, identity))
        {
            changed = true;
        }
        if let Some(evidence) = inputs.evidence.get(path_bytes(&original.path)) {
            for read in evidence {
                if read.before != read.after
                    || !graph_compatible(&read.after, &graph)
                    || read
                        .opened_identity
                        .is_some_and(|identity| !graph_has_identity(&graph, identity))
                {
                    changed = true;
                }
            }
        }
        final_rows.push(BuildInput {
            path: original.path.clone(),
            state,
        });
        final_graphs.push(graph);
    }
    enforce_graph_limit(
        final_rows
            .iter()
            .zip(&final_graphs)
            .map(|(input, graph)| (&input.path, graph)),
    )?;

    let mut alias_index = None;
    let mut repair_dependency = None;
    if !changed && let Some(output) = output {
        let output_graph = snapshot_graph(output)?;
        let output_identity = fs::symlink_metadata(output)
            .ok()
            .map(|metadata| identity(&metadata));
        for (index, input) in final_rows.iter().enumerate() {
            let exact = path_bytes(output) == path_bytes(&input.path);
            let node_path = final_graphs[index]
                .nodes
                .iter()
                .any(|node| path_bytes(&node.path) == path_bytes(output));
            let same_identity = output_identity
                .is_some_and(|value| graph_has_identity(&final_graphs[index], value));
            let mut folded_absent = false;
            if output_identity.is_none() {
                for node in &final_graphs[index].nodes {
                    if matches!(node.state, NodeState::Missing)
                        && absent_slots_alias(
                            output,
                            &node.path,
                            &input.path,
                            &output_graph,
                            &final_graphs[index],
                        )?
                    {
                        folded_absent = true;
                        break;
                    }
                }
            }
            if exact || node_path || same_identity || folded_absent {
                alias_index = Some(index);
                repair_dependency = Some(WatchRepairDependency {
                    path: output.to_path_buf(),
                    graph: output_graph,
                });
                break;
            }
        }
    }
    Ok(FinalizedWatchInputs {
        inputs: FinalBuildInputSet {
            rows: final_rows,
            graphs: final_graphs,
            changed_during_attempt: changed,
        },
        alias_index,
        repair_dependency,
    })
}

fn classify(path: &Path) -> (BuildInputState, Option<(u64, u64)>) {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return (BuildInputState::Missing, None);
        }
        Err(_) => return (BuildInputState::Unreadable, None),
    };
    if !metadata.is_file() {
        return (BuildInputState::NonRegular, Some(identity(&metadata)));
    }
    let len = metadata.len();
    let Ok(expected) = usize::try_from(len) else {
        return (BuildInputState::Unreadable, Some(identity(&metadata)));
    };
    let mut file = match open_observed(path) {
        Ok(file) => file,
        Err(_) => return (BuildInputState::Unreadable, Some(identity(&metadata))),
    };
    let opened = file.metadata().ok().map(|value| identity(&value));
    let mut stream = Hash128Stream::for_len(expected);
    let mut buffer = [0u8; HASH_BUFFER];
    let mut received = 0usize;
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                received = match received.checked_add(count) {
                    Some(value) => value,
                    None => return (BuildInputState::Unreadable, opened),
                };
                if !stream.update(&buffer[..count]) {
                    return (BuildInputState::Unreadable, opened);
                }
            }
            Err(_) => return (BuildInputState::Unreadable, opened),
        }
    }
    let Some(content_hash) = stream.finish() else {
        return (BuildInputState::Unreadable, opened);
    };
    if received != expected {
        return (BuildInputState::Unreadable, opened);
    }
    (BuildInputState::Regular { content_hash, len }, opened)
}

#[cfg(unix)]
fn open_observed(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(not(unix))]
fn open_observed(path: &Path) -> io::Result<File> {
    File::open(path)
}

fn snapshot_graph(path: &Path) -> Result<PathGraph, BuildInputTopologyError> {
    validate_absolute_path(path)?;
    let mut pending: VecDeque<OsString> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();
    let mut current = root_for(path);
    let mut nodes = Vec::new();
    validate_path(&current, false)?;
    let root_metadata = fs::symlink_metadata(&current).map_err(|error| {
        BuildInputTopologyError::new(format!(
            "cannot inspect path '{}': {error}",
            encode_watch_path(&current)
        ))
    })?;
    let root_id = identity(&root_metadata);
    let root = GraphNode {
        path: current.clone(),
        state: if root_metadata.is_dir() {
            NodeState::Directory {
                device: root_id.0,
                inode: root_id.1,
            }
        } else {
            NodeState::Other {
                device: root_id.0,
                inode: root_id.1,
            }
        },
    };
    let mut traversals = 0usize;
    let mut followed_states = std::collections::BTreeSet::new();
    while let Some(component) = pending.pop_front() {
        current.push(&component);
        validate_path(&current, false)?;
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                push_node(
                    &mut nodes,
                    GraphNode {
                        path: current.clone(),
                        state: NodeState::Missing,
                    },
                    path,
                )?;
                break;
            }
            Err(error) => {
                return Err(BuildInputTopologyError::new(format!(
                    "cannot inspect path '{}': {error}",
                    encode_watch_path(&current)
                )));
            }
        };
        let id = identity(&metadata);
        if metadata.file_type().is_symlink() {
            let Some(target) = read_link_validated(&current)? else {
                // The directory entry changed between nofollow metadata and readlink. Represent
                // that transition as a missing node so the enclosing comparison schedules a new
                // revision instead of turning ordinary registration churn into a watcher error.
                push_node(
                    &mut nodes,
                    GraphNode {
                        path: current.clone(),
                        state: NodeState::Missing,
                    },
                    path,
                )?;
                break;
            };
            let traversal_state = (
                current.clone(),
                pending.iter().cloned().collect::<Vec<OsString>>(),
            );
            push_node(
                &mut nodes,
                GraphNode {
                    path: current.clone(),
                    state: NodeState::Symlink {
                        device: id.0,
                        inode: id.1,
                        target: target.as_os_str().to_os_string(),
                    },
                },
                path,
            )?;
            if !followed_states.insert(traversal_state) {
                break;
            }
            traversals += 1;
            if traversals > MAX_SYMLINKS {
                return Err(BuildInputTopologyError::new(format!(
                    "too many symlink traversals for '{}' (maximum {MAX_SYMLINKS})",
                    encode_watch_path(path)
                )));
            }
            let mut replacement: VecDeque<OsString> = target
                .components()
                .filter_map(|part| match part {
                    Component::Normal(value) => Some(value.to_os_string()),
                    Component::ParentDir => Some(OsString::from("..")),
                    _ => None,
                })
                .collect();
            replacement.append(&mut pending);
            pending = replacement;
            if target.is_absolute() {
                current = root_for(&target);
            } else {
                current.pop();
            }
            continue;
        }
        let state = if metadata.is_dir() {
            NodeState::Directory {
                device: id.0,
                inode: id.1,
            }
        } else if metadata.is_file() {
            NodeState::Regular {
                device: id.0,
                inode: id.1,
            }
        } else {
            NodeState::Other {
                device: id.0,
                inode: id.1,
            }
        };
        push_node(
            &mut nodes,
            GraphNode {
                path: current.clone(),
                state,
            },
            path,
        )?;
    }
    Ok(PathGraph { root, nodes })
}

#[cfg(unix)]
fn read_link_validated(path: &Path) -> Result<Option<PathBuf>, BuildInputTopologyError> {
    let path_c = std::ffi::CString::new(path_bytes(path)).map_err(|_| {
        BuildInputTopologyError::new(format!(
            "path '{}' contains NUL byte",
            encode_watch_path(path)
        ))
    })?;
    let mut bytes = [0u8; HASH_BUFFER];
    // SAFETY: `path_c` is NUL-terminated and `bytes` is a writable fixed buffer.
    let count = unsafe { libc::readlink(path_c.as_ptr(), bytes.as_mut_ptr().cast(), bytes.len()) };
    if count == -1 {
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENOENT) | Some(libc::EINVAL)
        ) {
            return Ok(None);
        }
        return Err(BuildInputTopologyError::new(format!(
            "cannot read symlink '{}': {}",
            encode_watch_path(path),
            error
        )));
    }
    let count = usize::try_from(count).map_err(|_| {
        BuildInputTopologyError::new(format!(
            "cannot read symlink '{}': invalid target length",
            encode_watch_path(path)
        ))
    })?;
    if count == bytes.len() {
        return Err(BuildInputTopologyError::new(format!(
            "cannot read symlink '{}': target exceeds inspection buffer",
            encode_watch_path(path)
        )));
    }
    let bytes = &bytes[..count];
    if bytes.len() > MAX_PATH_BYTES {
        return Err(BuildInputTopologyError::new(format!(
            "path too long (maximum {MAX_PATH_BYTES} bytes; got {}; hash {})",
            bytes.len(),
            Hash128::of(bytes).to_hex()
        )));
    }
    Ok(Some(PathBuf::from(os_string_from_bytes(bytes.to_vec()))))
}

#[cfg(not(unix))]
fn read_link_validated(path: &Path) -> Result<Option<PathBuf>, BuildInputTopologyError> {
    let target = match fs::read_link(path) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(BuildInputTopologyError::new(format!(
                "cannot read symlink '{}': {error}",
                encode_watch_path(path)
            )));
        }
    };
    validate_path(&target, false)?;
    Ok(Some(target))
}

fn push_node(
    nodes: &mut Vec<GraphNode>,
    node: GraphNode,
    logical: &Path,
) -> Result<(), BuildInputTopologyError> {
    if nodes.len() == MAX_GRAPH_NODES {
        return Err(BuildInputTopologyError::new(format!(
            "too many path components while resolving '{}' (maximum {MAX_GRAPH_NODES})",
            encode_watch_path(logical)
        )));
    }
    nodes.push(node);
    Ok(())
}

fn enforce_graph_limit<'a>(
    graphs: impl IntoIterator<Item = (&'a PathBuf, &'a PathGraph)>,
) -> Result<(), BuildInputTopologyError> {
    let mut distinct = std::collections::BTreeSet::new();
    for (logical, graph) in graphs {
        for node in &graph.nodes {
            if distinct.insert(node.clone()) && distinct.len() > MAX_GRAPH_NODES {
                return Err(BuildInputTopologyError::new(format!(
                    "too many path components while resolving '{}' (maximum {MAX_GRAPH_NODES})",
                    encode_watch_path(logical)
                )));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn absent_slots_alias(
    output: &Path,
    right: &Path,
    logical: &Path,
    left_graph: &PathGraph,
    right_graph: &PathGraph,
) -> Result<bool, BuildInputTopologyError> {
    if path_bytes(output) == path_bytes(right) {
        return Ok(true);
    }
    let left = left_graph
        .nodes
        .last()
        .filter(|node| matches!(node.state, NodeState::Missing))
        .map(|node| node.path.as_path())
        .unwrap_or(output);
    if path_bytes(left) == path_bytes(right) {
        return Ok(true);
    }
    let Some(left_parent) = left.parent() else {
        return Ok(false);
    };
    let Some(right_parent) = right.parent() else {
        return Ok(false);
    };
    let Some(left_parent_id) = graph_directory_identity(left_graph, left_parent) else {
        return Ok(false);
    };
    let Some(right_parent_id) = graph_directory_identity(right_graph, right_parent) else {
        return Ok(false);
    };
    if left_parent_id != right_parent_id {
        return Ok(false);
    }
    let Some(left_name) = left.file_name() else {
        return Ok(false);
    };
    let Some(right_name) = right.file_name() else {
        return Ok(false);
    };
    if path_component_bytes(left_name) == path_component_bytes(right_name) {
        return Ok(true);
    }
    let left_directory = open_probe_parent(left_parent, left_parent_id, output, logical)?;
    // SAFETY: the retained descriptor names the live common parent directory.
    let name_max = unsafe { libc::fpathconf(left_directory.as_raw_fd(), libc::_PC_NAME_MAX) };
    if name_max <= 0 {
        return Err(collation_error(
            output,
            logical,
            "unavailable NAME_MAX".to_string(),
        ));
    }
    let limit = usize::try_from(name_max).unwrap_or(1_023).min(1_023);
    const PROBE_SUFFIX_LEN: usize = 36;
    let left_len = path_component_bytes(left_name)
        .len()
        .checked_add(PROBE_SUFFIX_LEN)
        .ok_or_else(|| collation_error(output, logical, "component length overflow".to_string()))?;
    let right_len = path_component_bytes(right_name)
        .len()
        .checked_add(PROBE_SUFFIX_LEN)
        .ok_or_else(|| collation_error(output, logical, "component length overflow".to_string()))?;
    let rejected = left_len.max(right_len);
    if rejected > limit {
        return Err(BuildInputTopologyError::new(format!(
            "collation probe '{}'/'{}': suffixed component too long (maximum {limit}; got {rejected})",
            encode_watch_path(output),
            encode_watch_path(logical)
        )));
    }
    for _ in 0..16u32 {
        let suffix = random_probe_suffix()
            .map_err(|error| collation_error(output, logical, format!("randomness: {error}")))?;
        debug_assert_eq!(suffix.len(), PROBE_SUFFIX_LEN);
        let left_probe = probe_name(left_name, &suffix);
        let right_probe = probe_name(right_name, &suffix);
        let left_c = c_component(&left_probe, output, logical)?;
        let right_c = c_component(&right_probe, output, logical)?;
        // SAFETY: the component is NUL-free and relative, and the retained descriptor is a
        // close-on-exec directory descriptor.
        let probe_fd = unsafe {
            libc::openat(
                left_directory.as_raw_fd(),
                left_c.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if probe_fd == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(collation_error(output, logical, error.to_string()));
        }
        // SAFETY: successful `openat` returned a uniquely owned descriptor.
        let probe = unsafe { File::from_raw_fd(probe_fd) };
        let probe_id =
            identity(&probe.metadata().map_err(|error| {
                collation_error(left, logical, format!("probe identity: {error}"))
            })?);
        let mut found = std::mem::MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: `found` is writable, `right_c` is a NUL-terminated relative component, and the
        // retained descriptor names the same common parent.
        let lookup = unsafe {
            libc::fstatat(
                left_directory.as_raw_fd(),
                right_c.as_ptr(),
                found.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        let outcome = if lookup == 0 {
            // SAFETY: successful `fstatat` initialized the record.
            let found = unsafe { found.assume_init() };
            if stat_identity(&found) == Some(probe_id) {
                Some(true)
            } else {
                None
            }
        } else {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                Some(false)
            } else {
                cleanup_probe_at(
                    &left_directory,
                    &left_c,
                    &left_probe,
                    probe,
                    probe_id,
                    output,
                    logical,
                )?;
                return Err(collation_error(output, logical, error.to_string()));
            }
        };
        cleanup_probe_at(
            &left_directory,
            &left_c,
            &left_probe,
            probe,
            probe_id,
            output,
            logical,
        )?;
        if let Some(alias) = outcome {
            return Ok(alias);
        }
    }
    Err(collation_error(
        output,
        logical,
        "exhausted private names".to_string(),
    ))
}

#[cfg(not(unix))]
fn absent_slots_alias(
    _left: &Path,
    _right: &Path,
    _logical: &Path,
    _left_graph: &PathGraph,
    _right_graph: &PathGraph,
) -> Result<bool, BuildInputTopologyError> {
    Err(BuildInputTopologyError::new(
        "collation probe is unsupported on this platform",
    ))
}

fn random_probe_suffix() -> io::Result<String> {
    let mut random = [0u8; 16];
    File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut random))?;
    let mut suffix = String::from(".aw-");
    use std::fmt::Write as _;
    for byte in random {
        let _ = write!(suffix, "{byte:02x}");
    }
    Ok(suffix)
}

#[cfg(unix)]
fn probe_name(name: &OsStr, suffix: &str) -> OsString {
    let mut bytes = path_component_bytes(name).to_vec();
    bytes.extend_from_slice(suffix.as_bytes());
    os_string_from_bytes(bytes)
}

#[cfg(unix)]
fn graph_directory_identity(graph: &PathGraph, path: &Path) -> Option<(u64, u64)> {
    graph
        .nodes
        .iter()
        .chain(std::iter::once(&graph.root))
        .find_map(|node| {
            if node.path != path {
                return None;
            }
            match node.state {
                NodeState::Directory { device, inode } => Some((device, inode)),
                _ => None,
            }
        })
}

#[cfg(unix)]
fn open_probe_parent(
    parent: &Path,
    expected: (u64, u64),
    output: &Path,
    logical: &Path,
) -> Result<File, BuildInputTopologyError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(parent)
        .map_err(|error| collation_error(output, logical, format!("parent open: {error}")))?;
    let actual =
        identity(&file.metadata().map_err(|error| {
            collation_error(output, logical, format!("parent identity: {error}"))
        })?);
    if actual != expected {
        return Err(collation_error(
            output,
            logical,
            "parent identity changed".to_string(),
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn c_component(
    component: &OsStr,
    output: &Path,
    logical: &Path,
) -> Result<std::ffi::CString, BuildInputTopologyError> {
    std::ffi::CString::new(path_component_bytes(component))
        .map_err(|_| collation_error(output, logical, "probe component contains NUL".to_string()))
}

#[cfg(unix)]
fn cleanup_probe_at(
    directory: &File,
    component: &std::ffi::CStr,
    display_component: &OsStr,
    probe: File,
    expected: (u64, u64),
    output: &Path,
    logical: &Path,
) -> Result<(), BuildInputTopologyError> {
    let mut current = std::mem::MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `current` is writable and `component` is a NUL-terminated relative component.
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            component.as_ptr(),
            current.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return Err(collation_cleanup_error(
            display_component,
            output,
            logical,
            "probe ownership lost",
        ));
    }
    // SAFETY: successful `fstatat` initialized the record.
    let current = unsafe { current.assume_init() };
    if stat_identity(&current) != Some(expected) {
        return Err(collation_cleanup_error(
            display_component,
            output,
            logical,
            "probe ownership lost",
        ));
    }
    // SAFETY: ownership was checked against the retained descriptor immediately above.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), component.as_ptr(), 0) } == -1 {
        return Err(collation_cleanup_error(
            display_component,
            output,
            logical,
            &io::Error::last_os_error().to_string(),
        ));
    }
    let metadata = probe.metadata().map_err(|error| {
        collation_cleanup_error(display_component, output, logical, &error.to_string())
    })?;
    if metadata.nlink() != 0 {
        return Err(collation_cleanup_error(
            display_component,
            output,
            logical,
            "probe link count is not zero",
        ));
    }
    drop(probe);
    Ok(())
}

#[cfg(unix)]
#[allow(clippy::useless_conversion)] // `dev_t` is signed on macOS and `u64` on Linux.
fn stat_identity(value: &libc::stat) -> Option<(u64, u64)> {
    Some((
        u64::try_from(value.st_dev).ok()?,
        u64::try_from(value.st_ino).ok()?,
    ))
}

#[cfg(unix)]
fn collation_error(output: &Path, logical: &Path, message: String) -> BuildInputTopologyError {
    BuildInputTopologyError::new(format!(
        "collation probe '{}'/'{}': {message}",
        encode_watch_path(output),
        encode_watch_path(logical)
    ))
}

#[cfg(unix)]
fn collation_cleanup_error(
    component: &OsStr,
    output: &Path,
    logical: &Path,
    message: &str,
) -> BuildInputTopologyError {
    BuildInputTopologyError::new(format!(
        "collation cleanup '{}' for '{}'/'{}': {message}",
        encode_bytes(path_component_bytes(component), true),
        encode_watch_path(output),
        encode_watch_path(logical)
    ))
}

fn graph_compatible(original: &PathGraph, current: &PathGraph) -> bool {
    original == current
}

fn graph_has_identity(graph: &PathGraph, identity: (u64, u64)) -> bool {
    graph.nodes.iter().any(|node| match node.state {
        NodeState::Directory { device, inode }
        | NodeState::Regular { device, inode }
        | NodeState::Symlink { device, inode, .. }
        | NodeState::Other { device, inode } => (device, inode) == identity,
        NodeState::Missing => false,
    })
}

#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn identity(_metadata: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}

fn root_for(path: &Path) -> PathBuf {
    path.components()
        .next()
        .and_then(|component| match component {
            Component::RootDir => Some(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
            Component::Prefix(prefix) => Some(PathBuf::from(prefix.as_os_str())),
            _ => None,
        })
        .unwrap_or_default()
}

fn validate_absolute_path(path: &Path) -> Result<(), BuildInputTopologyError> {
    validate_path(path, true)
}

fn validate_path(path: &Path, absolute: bool) -> Result<(), BuildInputTopologyError> {
    let bytes = path_bytes(path);
    if bytes.len() > MAX_PATH_BYTES {
        return Err(BuildInputTopologyError::new(format!(
            "path too long (maximum {MAX_PATH_BYTES} bytes; got {}; hash {})",
            bytes.len(),
            Hash128::of(bytes).to_hex()
        )));
    }
    if let Some(offset) = bytes.iter().position(|byte| *byte == 0) {
        return Err(BuildInputTopologyError::new(format!(
            "path '{}' contains NUL byte at offset {offset}",
            encode_watch_path(path)
        )));
    }
    if absolute && !path.is_absolute() {
        return Err(BuildInputTopologyError::new(format!(
            "path '{}' is not absolute",
            encode_watch_path(path)
        )));
    }
    Ok(())
}

fn too_many_evidence(path: &Path) -> BuildInputTopologyError {
    BuildInputTopologyError::new(format!(
        "too many read-time path components while reading '{}' (maximum {MAX_EVIDENCE_NODES})",
        encode_watch_path(path)
    ))
}

fn encode_watch_path(path: &Path) -> String {
    encode_bytes(path_bytes(path), true)
}

#[cfg(test)]
fn encode_watch_text(text: &str) -> String {
    if text.len() > 16_384 {
        return "message exceeds 16384-byte limit".to_string();
    }
    encode_bytes(text.as_bytes(), false)
}

fn encode_bytes(bytes: &[u8], path: bool) -> String {
    let mut output = String::with_capacity(bytes.len());
    for &byte in bytes {
        let keep = if path {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-')
        } else {
            (0x20..=0x7e).contains(&byte) && byte != b'%'
        };
        if keep {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(output, "%{byte:02X}");
        }
    }
    output
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> &[u8] {
    path.to_str().unwrap_or("").as_bytes()
}

#[cfg(unix)]
fn path_component_bytes(value: &OsStr) -> &[u8] {
    value.as_bytes()
}

#[cfg(not(unix))]
fn path_component_bytes(value: &OsStr) -> &[u8] {
    value.to_str().unwrap_or("").as_bytes()
}

#[cfg(unix)]
fn os_string_from_bytes(value: Vec<u8>) -> OsString {
    OsString::from_vec(value)
}

#[cfg(not(unix))]
fn os_string_from_bytes(value: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&value).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("align-watch-inputs-{}-{id}", std::process::id()));
            fs::create_dir(&path).expect("create temp directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn observation_merge_and_finalization_preserve_phase_and_order() {
        let temp = TempDir::new();
        let a = temp.0.join("a.align");
        let b = temp.0.join("b.align");
        fs::write(&a, b"a").expect("write a");
        fs::write(&b, b"b").expect("write b");
        let mut first = ObservationCollector::new();
        first.observe_path(&b).expect("observe b");
        first.observe_path(&a).expect("observe a");
        let first = first.finish().expect("first set");
        assert_eq!(first.inputs()[0].path(), a);
        assert_eq!(first.inputs()[1].path(), b);
        fs::write(&b, b"changed").expect("change b");
        let mut retry = ObservationCollector::new();
        retry.observe_path(&b).expect("observe changed b");
        let merged =
            merge_observed_build_inputs(first, retry.finish().expect("retry set")).expect("merge");
        assert!(merged.changed_during_attempt());
        let finalized = finalize_watch_inputs(merged, None).expect("finalize");
        assert!(finalized.inputs().changed_during_attempt());
        assert!(finalized.alias_index().is_none());
    }

    #[test]
    fn hard_link_output_is_rejected_and_repair_tracks_removal() {
        let temp = TempDir::new();
        let input = temp.0.join("input.align");
        let output = temp.0.join("output");
        fs::write(&input, b"source").expect("write input");
        fs::hard_link(&input, &output).expect("hard link output");
        let mut collector = ObservationCollector::new();
        collector.observe_path(&input).expect("observe input");
        let finalized = finalize_watch_inputs(collector.finish().expect("set"), Some(&output))
            .expect("finalize");
        assert_eq!(finalized.alias_index(), Some(0));
        let repair = finalized.repair_dependency().expect("repair dependency");
        fs::remove_file(&output).expect("remove alias");
        assert!(snapshot_watch_repair(&output).expect("new repair state") != *repair);
    }

    #[cfg(unix)]
    #[test]
    fn hard_linked_dangling_symlink_output_is_rejected_by_nofollow_identity() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let input = temp.0.join("input.align");
        let output = temp.0.join("output");
        symlink("missing.align", &input).expect("create dangling input symlink");
        fs::hard_link(&input, &output).expect("hard link dangling symlink");
        let mut collector = ObservationCollector::new();
        collector.observe_path(&input).expect("observe input");
        let finalized = finalize_watch_inputs(collector.finish().expect("set"), Some(&output))
            .expect("finalize dangling alias");
        assert_eq!(finalized.alias_index(), Some(0));
    }

    #[test]
    fn watch_codecs_are_reversible_and_bounded() {
        let raw = OsString::from_vec(vec![b'/', b'a', b'%', b'\n', 0xff]);
        assert_eq!(encode_watch_path(Path::new(&raw)), "/a%25%0A%FF");
        assert_eq!(encode_watch_text("marker\n%"), "marker%0A%25");
        assert_eq!(
            encode_watch_text(&"x".repeat(16_385)),
            "message exceeds 16384-byte limit"
        );
    }

    #[cfg(unix)]
    #[test]
    fn monitor_detects_same_bytes_symlink_retarget() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let first = temp.0.join("first.align");
        let second = temp.0.join("second.align");
        let logical = temp.0.join("main.align");
        fs::write(&first, b"same").expect("write first");
        fs::write(&second, b"same").expect("write second");
        symlink(&first, &logical).expect("create logical symlink");
        let mut collector = ObservationCollector::new();
        collector
            .observe_path(&logical)
            .expect("observe logical input");
        let finalized =
            finalize_watch_inputs(collector.finish().expect("set"), None).expect("finalize input");
        let mut baseline = monitor_baseline(finalized.inputs(), &[]).expect("monitor baseline");
        fs::remove_file(&logical).expect("remove first symlink");
        symlink(&second, &logical).expect("retarget logical symlink");
        assert!(
            refresh_monitor_baseline(&mut baseline).expect("refresh") == MonitorRefresh::Changed
        );
    }

    #[cfg(unix)]
    #[test]
    fn stable_symlink_loop_is_an_unreadable_input_not_a_topology_error() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let first = temp.0.join("first.align");
        let second = temp.0.join("second.align");
        symlink("second.align", &first).expect("create first loop edge");
        symlink("first.align", &second).expect("create second loop edge");
        let mut collector = ObservationCollector::new();
        assert!(matches!(
            collector.observe_path(&first).expect("observe loop"),
            BuildInputState::Unreadable
        ));
        let finalized =
            finalize_watch_inputs(collector.finish().expect("set"), None).expect("finalize loop");
        assert!(!finalized.inputs().changed_during_attempt());
        let mut baseline = monitor_baseline(finalized.inputs(), &[]).expect("monitor loop");
        assert!(
            refresh_monitor_baseline(&mut baseline).expect("refresh loop")
                == MonitorRefresh::Stable
        );
    }

    #[test]
    fn monitor_rearms_same_bytes_regular_inode_replacement() {
        let temp = TempDir::new();
        let logical = temp.0.join("main.align");
        let replacement = temp.0.join("replacement.align");
        fs::write(&logical, b"same").expect("write logical input");
        let mut collector = ObservationCollector::new();
        collector
            .observe_path(&logical)
            .expect("observe logical input");
        let finalized =
            finalize_watch_inputs(collector.finish().expect("set"), None).expect("finalize input");
        let mut baseline = monitor_baseline(finalized.inputs(), &[]).expect("monitor baseline");
        fs::write(&replacement, b"same").expect("write replacement");
        fs::rename(&replacement, &logical).expect("replace logical input");
        assert!(
            refresh_monitor_baseline(&mut baseline).expect("first refresh")
                == MonitorRefresh::Stable
        );
        assert!(
            refresh_monitor_baseline(&mut baseline).expect("second refresh")
                == MonitorRefresh::Stable
        );
    }

    #[test]
    fn monitor_rejects_a_hash_opened_from_the_pre_replacement_inode() {
        let temp = TempDir::new();
        let logical = temp.0.join("main.align");
        let replacement = temp.0.join("replacement.align");
        fs::write(&logical, b"old").expect("write logical input");
        let mut collector = ObservationCollector::new();
        collector
            .observe_path(&logical)
            .expect("observe logical input");
        let finalized =
            finalize_watch_inputs(collector.finish().expect("set"), None).expect("finalize input");
        let baseline = monitor_baseline(finalized.inputs(), &[]).expect("monitor baseline");

        let before = snapshot_graph(&logical).expect("pre-open graph");
        let (state, opened_identity) = classify(&logical);
        fs::write(&replacement, b"new").expect("write replacement");
        fs::rename(&replacement, &logical).expect("replace after open");
        let graph = snapshot_graph(&logical).expect("post-open graph");

        assert!(monitor_row_changed(
            &baseline.rows[0],
            &before,
            state,
            opened_identity,
            &graph,
        ));
    }

    #[test]
    fn distinct_absent_slots_leave_no_collation_probe() {
        let temp = TempDir::new();
        let input = temp.0.join("missing.align");
        let output = temp.0.join("program");
        let mut collector = ObservationCollector::new();
        collector
            .observe_path(&input)
            .expect("observe missing input");
        let finalized = finalize_watch_inputs(collector.finish().expect("set"), Some(&output))
            .expect("finalize distinct absent slots");
        assert!(finalized.alias_index().is_none());
        let residue = fs::read_dir(&temp.0)
            .expect("read temp directory")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.contains(".aw-"))
            .collect::<Vec<_>>();
        assert!(residue.is_empty(), "collation probe leaked: {residue:?}");
    }

    #[cfg(unix)]
    #[test]
    fn absent_output_through_parent_symlink_matches_the_resolved_input_slot() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let real = temp.0.join("real");
        let alias = temp.0.join("alias");
        fs::create_dir(&real).expect("create real parent");
        symlink(&real, &alias).expect("create parent alias");
        let input = real.join("program");
        let output = alias.join("program");
        let mut collector = ObservationCollector::new();
        collector
            .observe_path(&input)
            .expect("observe missing input");
        let finalized = finalize_watch_inputs(collector.finish().expect("set"), Some(&output))
            .expect("finalize aliased absent slot");
        assert_eq!(finalized.alias_index(), Some(0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn case_folded_absent_slots_are_aliases() {
        let temp = TempDir::new();
        let input = temp.0.join("Program");
        let output = temp.0.join("program");
        let mut collector = ObservationCollector::new();
        collector
            .observe_path(&input)
            .expect("observe missing input");
        let finalized = finalize_watch_inputs(collector.finish().expect("set"), Some(&output))
            .expect("finalize folded absent slots");
        assert_eq!(finalized.alias_index(), Some(0));
    }
}
