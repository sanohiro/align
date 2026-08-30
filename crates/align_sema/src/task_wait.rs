//! Path-complete, compiler-only proofs for `task_group` waits.
//!
//! The checker used to carry one boolean per open group.  This replay tracks the originating
//! group, Spawn generation, proof epoch, and every fallible Wait in source order.  It runs on the
//! already checked HIR before publication; no proof is serialized and no runtime ABI changes.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;

use align_ast::BinOp;
use align_diag::Diagnostics;
use align_span::Span;

use crate::hir::{Block, Expr, ExprKind, LocalId, MatchArm, Stmt};
use crate::{TaggedType, Ty, expand_tagged_ty, hir_expr_diverges};

type Token = u32;
type NodeId = u32;

const INITIAL_GENERATION: u8 = 1;
const INITIAL_EPOCH: u8 = 2;
const SPAWN_GENERATION: u8 = 3;
const SPAWN_EPOCH: u8 = 4;
const WAIT_TOKEN: u8 = 5;
const ERR_EPOCH: u8 = 6;
const JOIN_GENERATION: u8 = 7;
const JOIN_EPOCH: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TokenKey {
    group: NodeId,
    site: NodeId,
    kind: u8,
    incoming_generation: Token,
    incoming_epoch: Token,
}

#[derive(Default)]
struct Tokens {
    next: Token,
    values: HashMap<TokenKey, Token>,
}

impl Tokens {
    fn get(
        &mut self,
        group: NodeId,
        site: NodeId,
        kind: u8,
        incoming_generation: Token,
        incoming_epoch: Token,
    ) -> Option<Token> {
        let key = TokenKey {
            group,
            site,
            kind,
            incoming_generation,
            incoming_epoch,
        };
        if let Some(token) = self.values.get(&key) {
            return Some(*token);
        }
        let token = self.next.checked_add(1)?;
        self.next = token;
        self.values.insert(key, token);
        Some(token)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitStatus {
    Pending,
    Ok,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WaitRecord {
    status: WaitStatus,
    covers_through: Token,
    covered_generations: BTreeSet<Token>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    id: NodeId,
    generation: Token,
    epoch: Token,
    completed: Option<Token>,
    valid_generations: BTreeSet<Token>,
    fallible: bool,
    waits: BTreeMap<Token, WaitRecord>,
    wait_order: Vec<Token>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WaitProof {
    group: NodeId,
    epoch: Token,
    wait: Token,
    covers_through: Token,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TaskProof {
    group: NodeId,
    generation: Token,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Proof {
    Wait(WaitProof),
    Task(TaskProof),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    groups: Vec<Group>,
    waits: HashMap<LocalId, WaitProof>,
    tasks: HashMap<LocalId, TaskProof>,
}

#[derive(Clone, Debug)]
struct Flow {
    state: Option<State>,
    proof: Option<Proof>,
}

impl Flow {
    fn live(state: State, proof: Option<Proof>) -> Self {
        Self {
            state: Some(state),
            proof,
        }
    }

    fn dead() -> Self {
        Self {
            state: None,
            proof: None,
        }
    }

    fn clear_proof(mut self) -> Self {
        self.proof = None;
        self
    }
}

struct Analyzer<'a> {
    tagged_types: &'a [TaggedType],
    diags: &'a mut Diagnostics,
    node_ids: HashMap<usize, NodeId>,
    replay_steps: usize,
    max_replay_steps: usize,
    tokens: Tokens,
    loop_breaks: Vec<Vec<(State, Option<Proof>)>>,
    reported_gets: HashSet<NodeId>,
    replay_failed: bool,
    replay_failure_span: Option<Span>,
}

/// Explicit continuations for task-wait replay. A checked-HIR body is bounded by record depth,
/// but it may contain an arbitrary number of siblings. Sibling state lives in vectors owned by a
/// frame; nesting lives in this bounded work stack, not in the Rust call stack.
enum ReplayWork<'b> {
    EvalBlock {
        block: &'b Block,
        state: State,
        report: bool,
    },
    BlockAfterStmt {
        block: &'b Block,
        locals: Vec<LocalId>,
        next: usize,
        report: bool,
    },
    BlockAfterValue {
        locals: Vec<LocalId>,
    },
    EvalStmt {
        stmt: &'b Stmt,
        state: State,
        report: bool,
    },
    StmtAfterBinding {
        local: LocalId,
    },
    StmtAfterClear,
    StmtAfterIndexed {
        value: &'b Expr,
        report: bool,
    },
    StmtAfterReturn,
    StmtAfterBreak {
        accepted: bool,
    },
    EvalExpr {
        expr: &'b Expr,
        state: State,
        report: bool,
    },
    ExprAfterSpawn {
        expr: &'b Expr,
        fallible: bool,
    },
    ExprAfterTaskGet {
        site: NodeId,
        span: Span,
        report: bool,
    },
    ExprAfterTry,
    ExprAfterMapResult {
        f: &'b Expr,
        report: bool,
    },
    ExprAfterMapFunction {
        proof: Option<Proof>,
    },
    ExprAfterTaskGroup {
        group_id: NodeId,
    },
    ExprAfterElseOpt {
        site: NodeId,
        fallback: &'b Expr,
        opt_ty: Ty,
        report: bool,
    },
    ExprAfterElseFallback {
        site: NodeId,
        success: State,
        success_proof: Option<Proof>,
    },
    ExprAfterIfCondition {
        site: NodeId,
        then: &'b Block,
        els: &'b Block,
        report: bool,
    },
    ExprAfterIfThen {
        site: NodeId,
        els: &'b Block,
        else_state: State,
        report: bool,
    },
    ExprAfterIfElse {
        site: NodeId,
        then_flow: Flow,
    },
    ExprAfterMatchScrutinee {
        site: NodeId,
        arms: &'b [MatchArm],
        is_result: bool,
        report: bool,
    },
    MatchNext {
        site: NodeId,
        arms: Vec<(&'b Expr, State)>,
        next: usize,
        flows: Vec<Flow>,
        report: bool,
    },
    ExprAfterBinaryLhs {
        site: NodeId,
        rhs: &'b Expr,
        report: bool,
    },
    ExprAfterBinaryRhs {
        site: NodeId,
        lhs_state: State,
    },
    ExprAfterChildren {
        expr: &'b Expr,
        children: Vec<&'b Expr>,
        next: usize,
        report: bool,
    },
    LoopRun {
        site: NodeId,
        span: Span,
        body: &'b Block,
        body_locals: &'b Range<LocalId>,
        diverges: bool,
        entry: State,
        header: State,
        report: bool,
        final_pass: bool,
        steps: usize,
    },
    LoopAfterBody {
        site: NodeId,
        span: Span,
        body: &'b Block,
        body_locals: &'b Range<LocalId>,
        diverges: bool,
        entry: State,
        header: State,
        report: bool,
        final_pass: bool,
        steps: usize,
    },
}

const MAX_REPLAY_WORK: usize = crate::hir_depth::MAX_CHECKED_HIR_DEPTH * 8;
const MAX_LOOP_FIXED_POINT_STEPS: usize = crate::hir_depth::MAX_CHECKED_HIR_DEPTH * 8;

pub fn validate(body: &Block, tagged_types: &[TaggedType], diags: &mut Diagnostics) {
    if !crate::hir_depth::checked_hir_block_depth_is_valid(body) {
        return;
    }
    // Bodies without task-group constructs cannot contain a wait proof or a task-read error. Skip
    // the fixed-point replay so a deeply nested ordinary loop does not consume the task-specific
    // work budget at the checked-HIR boundary.
    if !contains_task_wait_construct(body) {
        return;
    }
    let Some((node_ids, record_count)) = collect_node_ids(body) else {
        return;
    };
    // `record_count` counts body events; replay starts with one synthetic root dispatcher item.
    let Some(max_replay_steps) = record_count
        .max(1)
        .checked_mul(MAX_REPLAY_WORK)
        .and_then(|bound| bound.checked_add(1))
    else {
        return;
    };
    let mut analyzer = Analyzer {
        tagged_types,
        diags,
        node_ids,
        replay_steps: 0,
        max_replay_steps,
        tokens: Tokens::default(),
        loop_breaks: Vec::new(),
        reported_gets: HashSet::new(),
        replay_failed: false,
        replay_failure_span: None,
    };
    let _ = analyzer.replay(body, State::default(), true);
    if analyzer.replay_failed {
        analyzer.diags.error(
            "task_group wait analysis exceeded its checked-HIR work bound".to_string(),
            analyzer
                .replay_failure_span
                .unwrap_or_else(|| Span::new(0, 0, 0)),
        );
    }
}

fn contains_task_wait_construct(body: &Block) -> bool {
    crate::hir_depth::body_events(body)
        .into_iter()
        .any(|event| {
            let crate::hir_depth::BodyEvent::ExprEnter(expression) = event else {
                return false;
            };
            matches!(
                expression.kind,
                ExprKind::Spawn { .. }
                    | ExprKind::TaskGet(_)
                    | ExprKind::TaskGroup(_)
                    | ExprKind::Wait
            )
        })
}

fn collect_node_ids(body: &Block) -> Option<(HashMap<usize, NodeId>, usize)> {
    let mut ids = HashMap::new();
    let mut next: NodeId = 1;
    let mut record_count = 0usize;
    for event in crate::hir_depth::body_events(body) {
        record_count = record_count.checked_add(1)?;
        let expression = match event {
            crate::hir_depth::BodyEvent::ExprEnter(expression) => Some(expression),
            crate::hir_depth::BodyEvent::StmtEnter(_)
            | crate::hir_depth::BodyEvent::StmtExit(_)
            | crate::hir_depth::BodyEvent::ExprExit { .. }
            | crate::hir_depth::BodyEvent::MatchArmEnter { .. } => None,
        };
        if let Some(expression) = expression {
            let key = expression as *const Expr as usize;
            if let std::collections::hash_map::Entry::Vacant(entry) = ids.entry(key) {
                entry.insert(next);
                next = next.checked_add(1)?;
            }
        }
    }
    Some((ids, record_count))
}

impl<'a> Analyzer<'a> {
    fn site(&mut self, expression: &Expr) -> Option<NodeId> {
        let Some(&id) = self.node_ids.get(&(expression as *const Expr as usize)) else {
            self.fail_replay(expression.span);
            return None;
        };
        Some(id)
    }

    fn fail_replay(&mut self, span: Span) {
        self.replay_failed = true;
        self.replay_failure_span.get_or_insert(span);
    }

    fn token(
        &mut self,
        group: NodeId,
        site: NodeId,
        kind: u8,
        incoming_generation: Token,
        incoming_epoch: Token,
    ) -> Option<Token> {
        let token = self
            .tokens
            .get(group, site, kind, incoming_generation, incoming_epoch);
        if token.is_none() {
            self.fail_replay(Span::new(0, 0, 0));
        }
        token
    }

    fn group<'b>(&self, state: &'b State, id: NodeId) -> Option<&'b Group> {
        state.groups.iter().rev().find(|group| group.id == id)
    }

    fn group_mut<'s>(&self, state: &'s mut State, id: NodeId) -> Option<&'s mut Group> {
        state.groups.iter_mut().rev().find(|group| group.id == id)
    }

    fn current_group(&self, state: &State) -> Option<NodeId> {
        state.groups.last().map(|group| group.id)
    }

    fn new_group(&mut self, id: NodeId) -> Option<Group> {
        let generation = self.token(id, id, INITIAL_GENERATION, 0, 0)?;
        let epoch = self.token(id, id, INITIAL_EPOCH, 0, 0)?;
        let mut valid_generations = BTreeSet::new();
        valid_generations.insert(generation);
        Some(Group {
            id,
            generation,
            epoch,
            completed: Some(generation),
            valid_generations,
            fallible: false,
            waits: BTreeMap::new(),
            wait_order: Vec::new(),
        })
    }

    fn task_ready(&self, state: &State, proof: TaskProof) -> bool {
        let Some(group) = self.group(state, proof.group) else {
            return false;
        };
        group.valid_generations.contains(&proof.generation)
            && group.completed == Some(group.generation)
    }

    fn task_merge_eligible(&self, state: &State, proof: TaskProof) -> bool {
        let Some(group) = self.group(state, proof.group) else {
            return false;
        };
        group.valid_generations.contains(&proof.generation)
            && (group.completed == Some(group.generation)
                || !group.waits.values().any(|wait| {
                    wait.status == WaitStatus::Pending
                        && wait.covered_generations.contains(&proof.generation)
                }))
    }

    fn wait_current(&self, state: &State, proof: WaitProof) -> bool {
        self.group(state, proof.group).is_some_and(|group| {
            group.epoch == proof.epoch
                && group
                    .waits
                    .get(&proof.wait)
                    .is_some_and(|record| record.covers_through == proof.covers_through)
        })
    }

    fn spawn(&mut self, state: &mut State, site: NodeId, fallible: bool) -> Option<TaskProof> {
        let id = self.current_group(state)?;
        let pending = self.group(state, id).is_some_and(|group| {
            group
                .waits
                .values()
                .any(|wait| wait.status == WaitStatus::Pending)
        });
        let (incoming_generation, incoming_epoch) = self
            .group(state, id)
            .map(|group| (group.generation, group.epoch))
            .unwrap_or((0, 0));
        let generation = self.token(
            id,
            site,
            SPAWN_GENERATION,
            incoming_generation,
            incoming_epoch,
        )?;
        let epoch = self.token(id, site, SPAWN_EPOCH, incoming_generation, incoming_epoch)?;
        let group = self.group_mut(state, id)?;
        if pending {
            group.valid_generations.clear();
        }
        group.generation = generation;
        group.epoch = epoch;
        group.waits.clear();
        group.wait_order.clear();
        group.valid_generations.insert(generation);
        group.fallible |= fallible;
        Some(TaskProof {
            group: id,
            generation,
        })
    }

    fn wait(&mut self, state: &mut State, site: NodeId, fallible: bool) -> Option<Proof> {
        let id = self.current_group(state)?;
        let (epoch, generation, covered) = {
            let group = self.group(state, id)?;
            (
                group.epoch,
                group.generation,
                group.valid_generations.clone(),
            )
        };
        if !fallible {
            self.group_mut(state, id)?.completed = Some(generation);
            return None;
        }
        let wait = self.token(id, site, WAIT_TOKEN, generation, epoch)?;
        if let Some(group) = self.group_mut(state, id) {
            if !group.waits.contains_key(&wait) {
                group.wait_order.push(wait);
            }
            group.waits.insert(
                wait,
                WaitRecord {
                    status: WaitStatus::Pending,
                    covers_through: generation,
                    covered_generations: covered,
                },
            );
        }
        Some(Proof::Wait(WaitProof {
            group: id,
            epoch,
            wait,
            covers_through: generation,
        }))
    }

    fn resolve_ok(&mut self, state: &mut State, proof: WaitProof) {
        let Some(group) = self.group_mut(state, proof.group) else {
            return;
        };
        if group.epoch != proof.epoch {
            return;
        }
        let Some(record) = group.waits.get_mut(&proof.wait) else {
            return;
        };
        if record.covers_through != proof.covers_through {
            return;
        }
        record.status = WaitStatus::Ok;
        let mut completed = group.completed;
        for wait in group.wait_order.iter().copied() {
            let Some(record) = group.waits.get(&wait) else {
                break;
            };
            if record.status != WaitStatus::Ok {
                break;
            }
            completed = Some(record.covers_through);
            if wait == proof.wait {
                break;
            }
        }
        group.completed = completed;
    }

    fn resolve_err(&mut self, state: &mut State, proof: WaitProof, site: NodeId) {
        let Some(snapshot) = self.group(state, proof.group).cloned() else {
            return;
        };
        if snapshot.epoch != proof.epoch {
            return;
        }
        let Some(record) = snapshot.waits.get(&proof.wait) else {
            return;
        };
        if record.covers_through != proof.covers_through {
            return;
        }
        if record.status == WaitStatus::Ok {
            return;
        }
        let Some(epoch) = self.token(
            proof.group,
            site,
            ERR_EPOCH,
            snapshot.generation,
            proof.epoch,
        ) else {
            return;
        };
        let Some(group) = self.group_mut(state, proof.group) else {
            return;
        };
        for generation in &record.covered_generations {
            group.valid_generations.remove(generation);
        }
        group.epoch = epoch;
        group.completed = None;
        group.waits.clear();
        group.wait_order.clear();
    }

    fn join_group(&mut self, site: NodeId, groups: &[Group]) -> Option<(Group, bool)> {
        let Some(first) = groups.first() else {
            let generation = self.token(site, site, JOIN_GENERATION, 0, 0)?;
            let epoch = self.token(site, site, JOIN_EPOCH, 0, 0)?;
            let mut valid_generations = BTreeSet::new();
            valid_generations.insert(generation);
            return Some((
                Group {
                    id: site,
                    generation,
                    epoch,
                    completed: None,
                    valid_generations,
                    fallible: false,
                    waits: BTreeMap::new(),
                    wait_order: Vec::new(),
                },
                true,
            ));
        };
        if groups.iter().all(|group| group == first) {
            return Some((first.clone(), false));
        }
        let generation = self.token(first.id, site, JOIN_GENERATION, 0, 0)?;
        let epoch = self.token(first.id, site, JOIN_EPOCH, 0, 0)?;
        let completed = groups
            .iter()
            .all(|group| group.completed == Some(group.generation));
        let mut valid_generations = BTreeSet::new();
        valid_generations.insert(generation);
        Some((
            Group {
                id: first.id,
                generation,
                epoch,
                completed: completed.then_some(generation),
                valid_generations,
                fallible: groups.iter().any(|group| group.fallible),
                waits: BTreeMap::new(),
                wait_order: Vec::new(),
            },
            true,
        ))
    }

    fn merge_states(&mut self, site: NodeId, states: &[State]) -> Option<State> {
        let first = states.first()?.clone();
        if states.len() == 1 {
            return Some(first);
        }
        let mut merged = first.clone();
        let mut changed = vec![false; first.groups.len()];
        for (index, changed_entry) in changed.iter_mut().enumerate() {
            let groups: Vec<Group> = states
                .iter()
                .filter_map(|state| state.groups.get(index).cloned())
                .collect();
            if groups.len() != states.len()
                || groups
                    .iter()
                    .any(|group| group.id != first.groups[index].id)
            {
                *changed_entry = true;
                continue;
            }
            let (group, differs) = self.join_group(site, &groups)?;
            merged.groups[index] = group;
            *changed_entry = differs;
        }

        merged.waits.clear();
        for (&local, proof) in &first.waits {
            if states
                .iter()
                .all(|state| state.waits.get(&local) == Some(proof))
                && self.wait_current(&merged, *proof)
            {
                merged.waits.insert(local, *proof);
            }
        }
        merged.tasks.clear();
        for (&local, proof) in &first.tasks {
            let proofs: Vec<TaskProof> = states
                .iter()
                .filter_map(|state| state.tasks.get(&local).copied())
                .collect();
            if proofs.len() != states.len()
                || !proofs
                    .iter()
                    .all(|candidate| candidate.group == proof.group)
                || !states
                    .iter()
                    .zip(&proofs)
                    .all(|(state, candidate)| self.task_merge_eligible(state, *candidate))
            {
                continue;
            }
            let Some(index) = merged
                .groups
                .iter()
                .position(|group| group.id == proof.group)
            else {
                continue;
            };
            let joined = if changed[index] {
                TaskProof {
                    group: proof.group,
                    generation: merged.groups[index].generation,
                }
            } else {
                *proofs.first().unwrap_or(proof)
            };
            merged.tasks.insert(local, joined);
        }
        Some(merged)
    }

    fn merge_flows(&mut self, site: NodeId, flows: Vec<Flow>) -> Flow {
        let live: Vec<(State, Option<Proof>)> = flows
            .into_iter()
            .filter_map(|flow| flow.state.map(|state| (state, flow.proof)))
            .collect();
        let Some((_, first_proof)) = live.first().cloned() else {
            return Flow::dead();
        };
        let states: Vec<State> = live.iter().map(|(state, _)| state.clone()).collect();
        let Some(merged) = self.merge_states(site, &states) else {
            return Flow::dead();
        };
        let proof = match first_proof {
            Some(Proof::Wait(proof))
                if live
                    .iter()
                    .all(|(_, candidate)| *candidate == Some(Proof::Wait(proof)))
                    && self.wait_current(&merged, proof) =>
            {
                Some(Proof::Wait(proof))
            }
            Some(Proof::Task(proof)) => {
                let proofs: Vec<TaskProof> = live
                    .iter()
                    .filter_map(|(_, candidate)| match candidate {
                        Some(Proof::Task(proof)) => Some(*proof),
                        _ => None,
                    })
                    .collect();
                let eligible = proofs.len() == live.len()
                    && proofs
                        .iter()
                        .all(|candidate| candidate.group == proof.group)
                    && live
                        .iter()
                        .zip(&proofs)
                        .all(|((state, _), candidate)| self.task_merge_eligible(state, *candidate));
                if eligible {
                    let Some(group) = merged.groups.iter().find(|group| group.id == proof.group)
                    else {
                        return Flow::live(merged, None);
                    };
                    Some(Proof::Task(TaskProof {
                        group: proof.group,
                        generation: group.generation,
                    }))
                } else {
                    None
                }
            }
            _ => None,
        };
        Flow::live(merged, proof)
    }

    fn get_error(
        &mut self,
        state: &State,
        proof: Option<Proof>,
        site: NodeId,
        span: Span,
        report: bool,
    ) {
        if !report || self.reported_gets.contains(&site) {
            return;
        }
        let Some(Proof::Task(task)) = proof else {
            self.reported_gets.insert(site);
            let fallible = state.groups.last().is_some_and(|group| group.fallible);
            let message = if fallible {
                "cannot call '.get()' before a successful 'wait()?' — this task_group is fallible, so use 'wait()?' to join (its error propagates) before reading results"
            } else {
                "cannot call '.get()' before 'wait()' — a task's result is ready only after the group is joined"
            };
            self.diags.error(message.to_string(), span);
            return;
        };
        if self.task_ready(state, task) {
            return;
        }
        self.reported_gets.insert(site);
        let message = self
            .group(state, task.group)
            .filter(|group| group.fallible)
            .map(|_| "cannot call '.get()' before a successful 'wait()?' — this task_group is fallible, so use 'wait()?' to join (its error propagates) before reading results")
            .unwrap_or("cannot call '.get()' before 'wait()' — a task's result is ready only after the group is joined");
        self.diags.error(message.to_string(), span);
    }

    fn replay(&mut self, block: &Block, state: State, report: bool) -> Flow {
        let mut work = vec![ReplayWork::EvalBlock {
            block,
            state,
            report,
        }];
        let mut last = Flow::dead();

        while let Some(item) = work.pop() {
            let Some(replay_steps) = self.replay_steps.checked_add(1) else {
                self.fail_replay(Span::new(0, 0, 0));
                return Flow::dead();
            };
            self.replay_steps = replay_steps;
            if replay_steps > self.max_replay_steps || work.len() > MAX_REPLAY_WORK {
                self.fail_replay(Span::new(0, 0, 0));
                return Flow::dead();
            }

            match item {
                ReplayWork::EvalBlock {
                    block,
                    state,
                    report,
                } => {
                    let locals = block_local_ids(block);
                    if let Some(stmt) = block.stmts.first() {
                        work.push(ReplayWork::BlockAfterStmt {
                            block,
                            locals,
                            next: 1,
                            report,
                        });
                        work.push(ReplayWork::EvalStmt {
                            stmt,
                            state,
                            report,
                        });
                    } else if let Some(value) = block.value.as_deref() {
                        work.push(ReplayWork::BlockAfterValue { locals });
                        work.push(ReplayWork::EvalExpr {
                            expr: value,
                            state,
                            report,
                        });
                    } else {
                        last = finish_block(Flow::live(state, None), locals);
                    }
                }
                ReplayWork::BlockAfterStmt {
                    block,
                    locals,
                    next,
                    report,
                } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    if let Some(stmt) = block.stmts.get(next) {
                        work.push(ReplayWork::BlockAfterStmt {
                            block,
                            locals,
                            next: next + 1,
                            report,
                        });
                        work.push(ReplayWork::EvalStmt {
                            stmt,
                            state,
                            report,
                        });
                    } else if let Some(value) = block.value.as_deref() {
                        work.push(ReplayWork::BlockAfterValue { locals });
                        work.push(ReplayWork::EvalExpr {
                            expr: value,
                            state,
                            report,
                        });
                    } else {
                        last = finish_block(Flow::live(state, None), locals);
                    }
                }
                ReplayWork::BlockAfterValue { locals } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    last = finish_block(flow, locals);
                }
                ReplayWork::EvalStmt {
                    stmt,
                    state,
                    report,
                } => match stmt {
                    Stmt::Let { local, init } => {
                        work.push(ReplayWork::StmtAfterBinding { local: *local });
                        work.push(ReplayWork::EvalExpr {
                            expr: init,
                            state,
                            report,
                        });
                    }
                    Stmt::Assign { local, value, .. } => {
                        work.push(ReplayWork::StmtAfterBinding { local: *local });
                        work.push(ReplayWork::EvalExpr {
                            expr: value,
                            state,
                            report,
                        });
                    }
                    Stmt::LetTuple { init, .. }
                    | Stmt::AssignField { value: init, .. }
                    | Stmt::AssignVecLane { value: init, .. }
                    | Stmt::TestAssert {
                        condition: init, .. }
                    | Stmt::Expr(init) => {
                        work.push(ReplayWork::StmtAfterClear);
                        work.push(ReplayWork::EvalExpr {
                            expr: init,
                            state,
                            report,
                        });
                    }
                    Stmt::AssignIndex { index, value, .. }
                    | Stmt::AssignElemField { index, value, .. }
                    | Stmt::AssignElem { index, value, .. } => {
                        work.push(ReplayWork::StmtAfterIndexed { value, report });
                        work.push(ReplayWork::EvalExpr {
                            expr: index,
                            state,
                            report,
                        });
                    }
                    Stmt::Return(Some(value)) => {
                        work.push(ReplayWork::StmtAfterReturn);
                        work.push(ReplayWork::EvalExpr {
                            expr: value,
                            state,
                            report,
                        });
                    }
                    Stmt::Return(None) => {
                        last = Flow::dead();
                    }
                    Stmt::Break { value, accepted } => {
                        if let Some(value) = value {
                            work.push(ReplayWork::StmtAfterBreak {
                                accepted: *accepted,
                            });
                            work.push(ReplayWork::EvalExpr {
                                expr: value,
                                state,
                                report,
                            });
                        } else {
                            if *accepted && let Some(breaks) = self.loop_breaks.last_mut() {
                                breaks.push((state, None));
                            }
                            last = Flow::dead();
                        }
                    }
                },
                ReplayWork::StmtAfterBinding { local } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(mut state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    state.waits.remove(&local);
                    state.tasks.remove(&local);
                    match flow.proof {
                        Some(Proof::Wait(proof)) => {
                            state.waits.insert(local, proof);
                        }
                        Some(Proof::Task(proof)) => {
                            state.tasks.insert(local, proof);
                        }
                        None => {}
                    }
                    last = Flow::live(state, None);
                }
                ReplayWork::StmtAfterClear => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    last = flow.clear_proof();
                }
                ReplayWork::StmtAfterIndexed { value, report } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    work.push(ReplayWork::StmtAfterClear);
                    work.push(ReplayWork::EvalExpr {
                        expr: value,
                        state,
                        report,
                    });
                }
                ReplayWork::StmtAfterReturn => {
                    let _ = std::mem::replace(&mut last, Flow::dead());
                    last = Flow::dead();
                }
                ReplayWork::StmtAfterBreak { accepted } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    if accepted && let Some(breaks) = self.loop_breaks.last_mut() {
                        breaks.push((state, flow.proof));
                    }
                    last = Flow::dead();
                }
                ReplayWork::EvalExpr {
                    expr,
                    state,
                    report,
                } => match &expr.kind {
                    ExprKind::Local(local) => {
                        last = Flow::live(
                            state.clone(),
                            state
                                .waits
                                .get(local)
                                .copied()
                                .map(Proof::Wait)
                                .or_else(|| state.tasks.get(local).copied().map(Proof::Task)),
                        );
                    }
                    ExprKind::Spawn { closure, fallible } => {
                        work.push(ReplayWork::ExprAfterSpawn {
                            expr,
                            fallible: *fallible,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr: closure,
                            state,
                            report,
                        });
                    }
                    ExprKind::Wait => {
                        let fallible =
                            matches!(expand_tagged_ty(expr.ty, self.tagged_types), Ty::Result(..));
                        let mut next = state;
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        let proof = self.wait(&mut next, site, fallible);
                        last = Flow::live(next, proof);
                    }
                    ExprKind::TaskGet(inner) => {
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        work.push(ReplayWork::ExprAfterTaskGet {
                            site,
                            span: expr.span,
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr: inner,
                            state,
                            report,
                        });
                    }
                    ExprKind::Try(inner) => {
                        work.push(ReplayWork::ExprAfterTry);
                        work.push(ReplayWork::EvalExpr {
                            expr: inner,
                            state,
                            report,
                        });
                    }
                    ExprKind::ResultMapErr { result, f } => {
                        work.push(ReplayWork::ExprAfterMapResult { f, report });
                        work.push(ReplayWork::EvalExpr {
                            expr: result,
                            state,
                            report,
                        });
                    }
                    ExprKind::ElseUnwrap { opt, fallback } => {
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        let opt_ty = expand_tagged_ty(opt.ty, self.tagged_types);
                        work.push(ReplayWork::ExprAfterElseOpt {
                            site,
                            fallback,
                            opt_ty,
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr: opt,
                            state,
                            report,
                        });
                    }
                    ExprKind::If { cond, then, els } => {
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        work.push(ReplayWork::ExprAfterIfCondition {
                            site,
                            then,
                            els,
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr: cond,
                            state,
                            report,
                        });
                    }
                    ExprKind::Match { scrutinee, arms, .. } => {
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        let is_result = matches!(
                            expand_tagged_ty(scrutinee.ty, self.tagged_types),
                            Ty::Result(..)
                        );
                        work.push(ReplayWork::ExprAfterMatchScrutinee {
                            site,
                            arms,
                            is_result,
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr: scrutinee,
                            state,
                            report,
                        });
                    }
                    ExprKind::Loop {
                        body,
                        body_locals,
                        diverges,
                    } => {
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        work.push(ReplayWork::LoopRun {
                            site,
                            span: expr.span,
                            body,
                            body_locals,
                            diverges: *diverges,
                            entry: state.clone(),
                            header: state,
                            report,
                            final_pass: false,
                            steps: 0,
                        });
                    }
                    ExprKind::Block(block)
                    | ExprKind::Arena(block)
                    | ExprKind::NamedArena { block, .. }
                    | ExprKind::Unsafe(block) => {
                        work.push(ReplayWork::EvalBlock {
                            block,
                            state,
                            report,
                        });
                    }
                    ExprKind::TaskGroup(block) => {
                        let Some(group_id) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        let mut nested = state;
                        let Some(group) = self.new_group(group_id) else {
                            last = Flow::dead();
                            continue;
                        };
                        nested.groups.push(group);
                        work.push(ReplayWork::ExprAfterTaskGroup { group_id });
                        work.push(ReplayWork::EvalBlock {
                            block,
                            state: nested,
                            report,
                        });
                    }
                    ExprKind::Binary {
                        op: BinOp::And | BinOp::Or,
                        lhs,
                        rhs,
                    } => {
                        let Some(site) = self.site(expr) else {
                            last = Flow::dead();
                            continue;
                        };
                        work.push(ReplayWork::ExprAfterBinaryLhs { site, rhs, report });
                        work.push(ReplayWork::EvalExpr {
                            expr: lhs,
                            state,
                            report,
                        });
                    }
                    _ => {
                        let children = crate::direct_expr_children(expr);
                        if let Some(child) = children.first().copied() {
                            work.push(ReplayWork::ExprAfterChildren {
                                expr,
                                children,
                                next: 1,
                                report,
                            });
                            work.push(ReplayWork::EvalExpr {
                                expr: child,
                                state,
                                report,
                            });
                        } else if hir_expr_diverges(expr) {
                            last = Flow::dead();
                        } else {
                            last = Flow::live(state, None);
                        }
                    }
                },
                ReplayWork::ExprAfterSpawn { expr, fallible } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(mut state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    let Some(site) = self.site(expr) else {
                        last = Flow::dead();
                        continue;
                    };
                    let task = self.spawn(&mut state, site, fallible);
                    last = Flow::live(state, task.map(Proof::Task));
                }
                ReplayWork::ExprAfterTaskGet { site, span, report } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    self.get_error(&state, flow.proof, site, span, report);
                    last = Flow::live(state, None);
                }
                ReplayWork::ExprAfterTry => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(mut state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    if let Some(Proof::Wait(proof)) = flow.proof {
                        self.resolve_ok(&mut state, proof);
                    }
                    last = Flow::live(state, None);
                }
                ReplayWork::ExprAfterMapResult { f, report } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    work.push(ReplayWork::ExprAfterMapFunction { proof: flow.proof });
                    work.push(ReplayWork::EvalExpr {
                        expr: f,
                        state,
                        report,
                    });
                }
                ReplayWork::ExprAfterMapFunction { proof } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    last = flow.map_proof(proof);
                }
                ReplayWork::ExprAfterTaskGroup { group_id } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let proof = flow.proof.filter(|proof| match proof {
                        Proof::Wait(wait) => wait.group != group_id,
                        Proof::Task(task) => task.group != group_id,
                    });
                    last = flow
                        .map_state(|mut state| {
                            state.waits.retain(|_, proof| proof.group != group_id);
                            state.tasks.retain(|_, proof| proof.group != group_id);
                            if let Some(index) =
                                state.groups.iter().rposition(|group| group.id == group_id)
                            {
                                state.groups.remove(index);
                            }
                            state
                        })
                        .map_proof(proof);
                }
                ReplayWork::ExprAfterElseOpt {
                    site,
                    fallback,
                    opt_ty,
                    report,
                } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(opt_state) = flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    let (mut success, mut failure) = (opt_state.clone(), opt_state);
                    let mut success_proof = flow.proof;
                    if let (Ty::Result(..), Some(Proof::Wait(proof))) = (opt_ty, flow.proof) {
                        self.resolve_ok(&mut success, proof);
                        self.resolve_err(&mut failure, proof, site);
                        success_proof = None;
                    }
                    work.push(ReplayWork::ExprAfterElseFallback {
                        site,
                        success,
                        success_proof,
                    });
                    work.push(ReplayWork::EvalExpr {
                        expr: fallback,
                        state: failure,
                        report,
                    });
                }
                ReplayWork::ExprAfterElseFallback {
                    site,
                    success,
                    success_proof,
                } => {
                    let fallback = std::mem::replace(&mut last, Flow::dead());
                    last =
                        self.merge_flows(site, vec![Flow::live(success, success_proof), fallback]);
                }
                ReplayWork::ExprAfterIfCondition {
                    site,
                    then,
                    els,
                    report,
                } => {
                    let condition = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = condition.state else {
                        last = Flow::dead();
                        continue;
                    };
                    work.push(ReplayWork::ExprAfterIfThen {
                        site,
                        els,
                        else_state: state.clone(),
                        report,
                    });
                    work.push(ReplayWork::EvalBlock {
                        block: then,
                        state,
                        report,
                    });
                }
                ReplayWork::ExprAfterIfThen {
                    site,
                    els,
                    else_state,
                    report,
                } => {
                    let then_flow = std::mem::replace(&mut last, Flow::dead());
                    work.push(ReplayWork::ExprAfterIfElse { site, then_flow });
                    work.push(ReplayWork::EvalBlock {
                        block: els,
                        state: else_state,
                        report,
                    });
                }
                ReplayWork::ExprAfterIfElse { site, then_flow } => {
                    let else_flow = std::mem::replace(&mut last, Flow::dead());
                    last = self.merge_flows(site, vec![then_flow, else_flow]);
                }
                ReplayWork::ExprAfterMatchScrutinee {
                    site,
                    arms,
                    is_result,
                    report,
                } => {
                    let scrutinee_flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(base) = scrutinee_flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    let mut remaining = BTreeSet::from([0_u32, 1_u32]);
                    let mut arm_plans = Vec::new();
                    for arm in arms {
                        let tags: Vec<u32> = if is_result {
                            if arm.variants.is_empty() {
                                remaining.iter().copied().collect()
                            } else {
                                arm.variants.clone()
                            }
                        } else {
                            vec![0]
                        };
                        if is_result {
                            for tag in &tags {
                                remaining.remove(tag);
                            }
                        }
                        if tags.is_empty() {
                            continue;
                        }
                        let mut arm_state = base.clone();
                        if let Some(Proof::Wait(proof)) = scrutinee_flow.proof {
                            if is_result && tags.len() == 1 && tags[0] == 0 {
                                self.resolve_ok(&mut arm_state, proof);
                            } else if is_result && tags.len() == 1 && tags[0] == 1 {
                                self.resolve_err(&mut arm_state, proof, site);
                            }
                        }
                        arm_plans.push((&arm.body, arm_state));
                    }
                    if arm_plans.is_empty() {
                        last = Flow::live(base, None);
                    } else {
                        let (expr, state) = arm_plans[0].clone();
                        work.push(ReplayWork::MatchNext {
                            site,
                            arms: arm_plans,
                            next: 1,
                            flows: Vec::new(),
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr,
                            state,
                            report,
                        });
                    }
                }
                ReplayWork::MatchNext {
                    site,
                    arms,
                    next,
                    mut flows,
                    report,
                } => {
                    flows.push(std::mem::replace(&mut last, Flow::dead()));
                    if let Some((expr, state)) = arms.get(next).cloned() {
                        work.push(ReplayWork::MatchNext {
                            site,
                            arms,
                            next: next + 1,
                            flows,
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr,
                            state,
                            report,
                        });
                    } else {
                        last = self.merge_flows(site, flows);
                    }
                }
                ReplayWork::ExprAfterBinaryLhs { site, rhs, report } => {
                    let lhs_flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(lhs_state) = lhs_flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    work.push(ReplayWork::ExprAfterBinaryRhs {
                        site,
                        lhs_state: lhs_state.clone(),
                    });
                    work.push(ReplayWork::EvalExpr {
                        expr: rhs,
                        state: lhs_state,
                        report,
                    });
                }
                ReplayWork::ExprAfterBinaryRhs { site, lhs_state } => {
                    let rhs_flow = std::mem::replace(&mut last, Flow::dead());
                    last = self.merge_flows(site, vec![Flow::live(lhs_state, None), rhs_flow]);
                }
                ReplayWork::ExprAfterChildren {
                    expr,
                    children,
                    next,
                    report,
                } => {
                    let child_flow = std::mem::replace(&mut last, Flow::dead());
                    let Some(state) = child_flow.state else {
                        last = Flow::dead();
                        continue;
                    };
                    if let Some(child) = children.get(next).copied() {
                        work.push(ReplayWork::ExprAfterChildren {
                            expr,
                            children,
                            next: next + 1,
                            report,
                        });
                        work.push(ReplayWork::EvalExpr {
                            expr: child,
                            state,
                            report,
                        });
                    } else if hir_expr_diverges(expr) {
                        last = Flow::dead();
                    } else {
                        last = Flow::live(state, None);
                    }
                }
                ReplayWork::LoopRun {
                    site,
                    span,
                    body,
                    body_locals,
                    diverges,
                    entry,
                    header,
                    report,
                    final_pass,
                    steps,
                } => {
                    self.loop_breaks.push(Vec::new());
                    work.push(ReplayWork::LoopAfterBody {
                        site,
                        span,
                        body,
                        body_locals,
                        diverges,
                        entry,
                        header: header.clone(),
                        report,
                        final_pass,
                        steps,
                    });
                    work.push(ReplayWork::EvalBlock {
                        block: body,
                        state: header,
                        report: if final_pass { report } else { false },
                    });
                }
                ReplayWork::LoopAfterBody {
                    site,
                    span,
                    body,
                    body_locals,
                    diverges,
                    entry,
                    header,
                    report,
                    final_pass,
                    steps,
                } => {
                    let flow = std::mem::replace(&mut last, Flow::dead());
                    let breaks = self.loop_breaks.pop().unwrap_or_default();
                    if final_pass {
                        if diverges || breaks.is_empty() {
                            last = Flow::dead();
                        } else {
                            let break_flows = breaks
                                .into_iter()
                                .map(|(mut state, proof)| {
                                    clear_locals(&mut state, body_locals);
                                    Flow::live(state, proof)
                                })
                                .collect();
                            last = self.merge_flows(site, break_flows);
                        }
                        continue;
                    }
                    let mut predecessors = vec![entry.clone()];
                    if let Some(fallthrough) = flow.state {
                        predecessors.push(fallthrough);
                    }
                    let next = self
                        .merge_states(site, &predecessors)
                        .unwrap_or_else(|| entry.clone());
                    if next == header {
                        work.push(ReplayWork::LoopRun {
                            site,
                            span,
                            body,
                            body_locals,
                            diverges,
                            entry,
                            header,
                            report,
                            final_pass: true,
                            steps,
                        });
                    } else if steps >= MAX_LOOP_FIXED_POINT_STEPS {
                        self.fail_replay(span);
                        last = Flow::dead();
                    } else {
                        work.push(ReplayWork::LoopRun {
                            site,
                            span,
                            body,
                            body_locals,
                            diverges,
                            entry,
                            header: next,
                            report,
                            final_pass: false,
                            steps: steps + 1,
                        });
                    }
                }
            }
        }
        last
    }
}

trait FlowExt {
    fn map_proof(self, proof: Option<Proof>) -> Flow;
    fn map_state<F: FnOnce(State) -> State>(self, f: F) -> Flow;
}

impl FlowExt for Flow {
    fn map_proof(mut self, proof: Option<Proof>) -> Flow {
        if self.state.is_some() {
            self.proof = proof;
        }
        self
    }

    fn map_state<F: FnOnce(State) -> State>(mut self, f: F) -> Flow {
        if let Some(state) = self.state.take() {
            self.state = Some(f(state));
        }
        self
    }
}

fn finish_block(flow: Flow, locals: Vec<LocalId>) -> Flow {
    if let Some(mut state) = flow.state.clone() {
        for local in locals {
            state.waits.remove(&local);
            state.tasks.remove(&local);
        }
        Flow::live(state, flow.proof)
    } else {
        flow
    }
}

fn clear_locals(state: &mut State, locals: &Range<LocalId>) {
    state.waits.retain(|local, _| !locals.contains(local));
    state.tasks.retain(|local, _| !locals.contains(local));
}

fn block_local_ids(block: &Block) -> Vec<LocalId> {
    let mut locals = Vec::new();
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { local, .. } => locals.push(*local),
            Stmt::LetTuple {
                locals: tuple_locals,
                ..
            } => locals.extend(tuple_locals.iter().flatten().copied()),
            _ => {}
        }
    }
    locals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntTy, Scalar};

    fn unit_expr(span: Span) -> Expr {
        Expr {
            kind: ExprKind::Unit,
            ty: Ty::Unit,
            span,
        }
    }

    fn fallible_wait_expr(span: Span) -> Expr {
        Expr {
            kind: ExprKind::Wait,
            ty: Ty::Result(
                Scalar::Unit,
                Scalar::Int(IntTy {
                    bits: 32,
                    signed: true,
                }),
            ),
            span,
        }
    }

    fn test_analyzer<'a>(
        tagged_types: &'a [TaggedType],
        diags: &'a mut Diagnostics,
        node_ids: HashMap<usize, NodeId>,
    ) -> Analyzer<'a> {
        Analyzer {
            tagged_types,
            diags,
            node_ids,
            replay_steps: 0,
            max_replay_steps: usize::MAX,
            tokens: Tokens::default(),
            loop_breaks: Vec::new(),
            reported_gets: HashSet::new(),
            replay_failed: false,
            replay_failure_span: None,
        }
    }

    fn nested_block_body(count: usize) -> Block {
        let span = Span::new(0, 0, 0);
        let mut value = unit_expr(span);
        for _ in 0..count {
            value = Expr {
                kind: ExprKind::Block(Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(value)),
                }),
                ty: Ty::Unit,
                span,
            };
        }
        Block {
            stmts: Vec::new(),
            value: Some(Box::new(value)),
        }
    }

    fn nested_statement_body(count: usize) -> Block {
        let span = Span::new(0, 0, 0);
        let mut value = unit_expr(span);
        for _ in 0..count {
            value = Expr {
                kind: ExprKind::Block(Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(value)),
                }),
                ty: Ty::Unit,
                span,
            };
        }
        Block {
            stmts: vec![Stmt::Expr(value)],
            value: None,
        }
    }

    fn spawn_expr(span: Span) -> Expr {
        Expr {
            kind: ExprKind::Spawn {
                closure: Box::new(unit_expr(span)),
                fallible: false,
            },
            ty: Ty::Unit,
            span,
        }
    }

    fn bool_expr(span: Span) -> Expr {
        Expr {
            kind: ExprKind::Bool(true),
            ty: Ty::Bool,
            span,
        }
    }

    fn identity_group(span: Span) -> Expr {
        let error_branch = Expr {
            kind: ExprKind::ElseUnwrap {
                opt: Box::new(fallible_wait_expr(span)),
                fallback: Box::new(unit_expr(span)),
            },
            ty: Ty::Unit,
            span,
        };
        let branch = Expr {
            kind: ExprKind::If {
                cond: Box::new(bool_expr(span)),
                then: Block {
                    stmts: vec![Stmt::Expr(spawn_expr(span))],
                    value: None,
                },
                els: Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(unit_expr(span))),
                },
            },
            ty: Ty::Unit,
            span,
        };
        let loop_body = Block {
            stmts: vec![
                Stmt::Expr(spawn_expr(span)),
                Stmt::Expr(Expr {
                    kind: ExprKind::If {
                        cond: Box::new(bool_expr(span)),
                        then: Block {
                            stmts: vec![Stmt::Break {
                                value: None,
                                accepted: true,
                            }],
                            value: None,
                        },
                        els: Block {
                            stmts: Vec::new(),
                            value: Some(Box::new(unit_expr(span))),
                        },
                    },
                    ty: Ty::Unit,
                    span,
                }),
            ],
            value: None,
        };
        Expr {
            kind: ExprKind::TaskGroup(Block {
                stmts: vec![
                    Stmt::Expr(spawn_expr(span)),
                    Stmt::Expr(spawn_expr(span)),
                    Stmt::Expr(fallible_wait_expr(span)),
                    Stmt::Expr(error_branch),
                    Stmt::Expr(branch),
                    Stmt::Expr(Expr {
                        kind: ExprKind::Loop {
                            body: loop_body,
                            body_locals: 0..0,
                            diverges: false,
                        },
                        ty: Ty::Unit,
                        span,
                    }),
                ],
                value: None,
            }),
            ty: Ty::Unit,
            span,
        }
    }

    fn identity_loop_sites(group: &Expr) -> (&Expr, &Expr) {
        let ExprKind::TaskGroup(block) = &group.kind else {
            unreachable!("identity fixture must be a task group");
        };
        let Stmt::Expr(loop_expr) = &block.stmts[5] else {
            unreachable!("identity fixture must end with a loop");
        };
        let ExprKind::Loop { body, .. } = &loop_expr.kind else {
            unreachable!("identity fixture must end with a loop");
        };
        let Stmt::Expr(spawn_expr) = &body.stmts[0] else {
            unreachable!("loop fixture must start with a Spawn");
        };
        (loop_expr, spawn_expr)
    }

    #[test]
    fn task_wait_duplicate_span_identity() {
        let span = Span::new(7, 11, 11);
        let body = Block {
            stmts: vec![Stmt::Expr(Expr {
                kind: ExprKind::TaskGroup(Block {
                    stmts: vec![
                        Stmt::Expr(fallible_wait_expr(span)),
                        Stmt::Expr(fallible_wait_expr(span)),
                    ],
                    value: None,
                }),
                ty: Ty::Unit,
                span,
            })],
            value: None,
        };
        let (ids, _) = collect_node_ids(&body).expect("valid body has stable node ids");
        let group = match &body.stmts[0] {
            Stmt::Expr(expr) => expr,
            _ => unreachable!(),
        };
        let (first, second) = match &group.kind {
            ExprKind::TaskGroup(block) => match (&block.stmts[0], &block.stmts[1]) {
                (Stmt::Expr(first), Stmt::Expr(second)) => (first, second),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        assert_ne!(
            ids.get(&(first as *const Expr as usize)),
            ids.get(&(second as *const Expr as usize)),
            "equal spans must not alias distinct structural nodes"
        );
        let group_id = ids[&(group as *const Expr as usize)];
        let first_id = ids[&(first as *const Expr as usize)];
        let second_id = ids[&(second as *const Expr as usize)];
        assert!(group_id < first_id && first_id < second_id);

        let mut diagnostics = Diagnostics::new();
        let mut analyzer = test_analyzer(&[], &mut diagnostics, ids);
        let flow = analyzer.replay(&body, State::default(), true);
        assert!(flow.state.is_some());
        let wait_sites: Vec<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == WAIT_TOKEN)
            .map(|key| key.site)
            .collect();
        assert_eq!(
            wait_sites.len(),
            2,
            "duplicate spans must produce two wait tokens"
        );
        assert_ne!(wait_sites[0], wait_sites[1]);
        assert!(!analyzer.replay_failed);
    }

    #[test]
    fn task_wait_duplicate_span_all_identity_kinds() {
        let span = Span::new(13, 17, 17);
        let body = Block {
            stmts: vec![
                Stmt::Expr(identity_group(span)),
                Stmt::Expr(identity_group(span)),
            ],
            value: None,
        };
        let (node_ids, _) = collect_node_ids(&body).expect("valid body has stable node ids");
        let loop_ids: std::collections::BTreeSet<NodeId> = body
            .stmts
            .iter()
            .map(|stmt| {
                let Stmt::Expr(group) = stmt else {
                    unreachable!("identity fixture statement must be an expression");
                };
                let (loop_expr, _) = identity_loop_sites(group);
                node_ids[&(loop_expr as *const Expr as usize)]
            })
            .collect();
        let mut diagnostics = Diagnostics::new();
        let mut analyzer = test_analyzer(&[], &mut diagnostics, node_ids);
        let flow = analyzer.replay(&body, State::default(), true);
        assert!(flow.state.is_some());
        assert!(!analyzer.replay_failed);

        let initial_groups: std::collections::BTreeSet<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == INITIAL_GENERATION && key.group == key.site)
            .map(|key| key.group)
            .collect();
        let spawn_sites: std::collections::BTreeSet<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == SPAWN_GENERATION)
            .map(|key| key.site)
            .collect();
        let err_sites: std::collections::BTreeSet<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == ERR_EPOCH)
            .map(|key| key.site)
            .collect();
        let join_sites: std::collections::BTreeSet<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == JOIN_GENERATION)
            .map(|key| key.site)
            .collect();
        assert_eq!(initial_groups.len(), 2, "duplicate groups must not alias");
        assert_eq!(spawn_sites.len(), 8, "duplicate Spawn sites must not alias");
        assert_eq!(err_sites.len(), 2, "duplicate Err sites must not alias");
        assert!(join_sites.len() >= 4, "duplicate join sites must not alias");
        let loop_join_sites: std::collections::BTreeSet<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == JOIN_GENERATION && loop_ids.contains(&key.site))
            .map(|key| key.site)
            .collect();
        assert_eq!(
            loop_join_sites, loop_ids,
            "loop headers must retain identity"
        );
    }

    #[test]
    fn task_wait_missing_node_fails_closed() {
        let body = Block {
            stmts: vec![Stmt::Expr(fallible_wait_expr(Span::new(0, 0, 1)))],
            value: None,
        };
        let mut diagnostics = Diagnostics::new();
        let mut analyzer = test_analyzer(&[], &mut diagnostics, HashMap::new());
        let flow = analyzer.replay(&body, State::default(), true);
        assert!(flow.state.is_none());
        assert!(analyzer.replay_failed);
    }

    #[test]
    fn task_wait_token_exhaustion_fails_closed() {
        let span = Span::new(0, 0, 1);
        let body = Block {
            stmts: vec![Stmt::Expr(Expr {
                kind: ExprKind::TaskGroup(Block {
                    stmts: Vec::new(),
                    value: None,
                }),
                ty: Ty::Unit,
                span,
            })],
            value: None,
        };
        let (node_ids, _) = collect_node_ids(&body).expect("valid body has stable node ids");
        let mut diagnostics = Diagnostics::new();
        let mut analyzer = test_analyzer(&[], &mut diagnostics, node_ids);
        analyzer.tokens.next = Token::MAX;
        let flow = analyzer.replay(&body, State::default(), true);
        assert!(flow.state.is_none());
        assert!(analyzer.replay_failed);
    }

    #[test]
    fn task_wait_duplicate_span_gets_report_separately() {
        let span = Span::new(3, 7, 7);
        let task_get = || Expr {
            kind: ExprKind::TaskGet(Box::new(Expr {
                kind: ExprKind::Local(0),
                ty: Ty::Unit,
                span,
            })),
            ty: Ty::Int(IntTy {
                bits: 32,
                signed: true,
            }),
            span,
        };
        let body = Block {
            stmts: vec![Stmt::Expr(task_get()), Stmt::Expr(task_get())],
            value: None,
        };
        let mut diagnostics = Diagnostics::new();
        validate(&body, &[], &mut diagnostics);
        assert_eq!(
            diagnostics.error_count(),
            2,
            "distinct invalid TaskGet nodes must not deduplicate by span"
        );
    }

    #[test]
    fn task_wait_empty_body_has_replay_budget() {
        let body = Block {
            stmts: Vec::new(),
            value: None,
        };
        let mut diagnostics = Diagnostics::new();
        validate(&body, &[], &mut diagnostics);
        assert!(!diagnostics.has_errors());
    }

    #[test]
    fn task_wait_depth_is_stack_bounded() {
        let body = nested_statement_body((crate::hir_depth::MAX_CHECKED_HIR_DEPTH - 3) / 2);
        assert!(crate::hir_depth::checked_hir_block_depth_is_valid(&body));
        std::thread::Builder::new()
            .name("task-wait-depth".to_string())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let mut diagnostics = Diagnostics::new();
                validate(&body, &[], &mut diagnostics);
                assert!(
                    !diagnostics.has_errors(),
                    "in-bound task-wait replay must not fail: {:?}",
                    diagnostics.iter().collect::<Vec<_>>()
                );
            })
            .expect("spawn task-wait depth owner")
            .join()
            .expect("task-wait depth owner");

        let over = nested_block_body(crate::hir_depth::MAX_CHECKED_HIR_DEPTH / 2);
        assert!(!crate::hir_depth::checked_hir_block_depth_is_valid(&over));
    }

    #[test]
    fn task_wait_loop_fixed_point_guard_is_depth_derived() {
        assert_eq!(
            MAX_LOOP_FIXED_POINT_STEPS,
            crate::hir_depth::MAX_CHECKED_HIR_DEPTH * 8
        );
        let span = Span::new(0, 0, 1);
        let body = Block {
            stmts: vec![Stmt::Expr(identity_group(span))],
            value: None,
        };
        let group = match &body.stmts[0] {
            Stmt::Expr(group) => group,
            _ => unreachable!(),
        };
        let (loop_expr, loop_spawn) = identity_loop_sites(group);
        let (node_ids, _) = collect_node_ids(&body).expect("valid body has stable node ids");
        let loop_site = node_ids[&(loop_expr as *const Expr as usize)];
        let loop_spawn_site = node_ids[&(loop_spawn as *const Expr as usize)];
        let mut diagnostics = Diagnostics::new();
        let mut analyzer = test_analyzer(&[], &mut diagnostics, node_ids);
        let flow = analyzer.replay(&body, State::default(), true);
        assert!(flow.state.is_some());
        assert!(!analyzer.diags.has_errors());
        let loop_spawn_inputs: std::collections::BTreeSet<(Token, Token)> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == SPAWN_GENERATION && key.site == loop_spawn_site)
            .map(|key| (key.incoming_generation, key.incoming_epoch))
            .collect();
        assert!(
            loop_spawn_inputs.len() >= 2,
            "loop header must be recomputed after its first state-changing pass"
        );
        let loop_join_sites: std::collections::BTreeSet<NodeId> = analyzer
            .tokens
            .values
            .keys()
            .filter(|key| key.kind == JOIN_GENERATION && key.site == loop_site)
            .map(|key| key.site)
            .collect();
        assert_eq!(loop_join_sites, [loop_site].into_iter().collect());
    }

    #[test]
    fn task_wait_loop_unresolved_wait_reaches_later_break() {
        let span = Span::new(0, 0, 1);
        let task = Expr {
            kind: ExprKind::Spawn {
                closure: Box::new(unit_expr(span)),
                fallible: true,
            },
            ty: Ty::Unit,
            span,
        };
        let wait = || fallible_wait_expr(span);
        let loop_body = Block {
            stmts: vec![
                Stmt::Expr(wait()),
                Stmt::Break {
                    value: None,
                    accepted: true,
                },
            ],
            value: None,
        };
        let group = Expr {
            kind: ExprKind::TaskGroup(Block {
                stmts: vec![
                    Stmt::Let {
                        local: 0,
                        init: task,
                    },
                    Stmt::Expr(Expr {
                        kind: ExprKind::Loop {
                            body: loop_body,
                            body_locals: 1..1,
                            diverges: false,
                        },
                        ty: Ty::Unit,
                        span,
                    }),
                    Stmt::Expr(Expr {
                        kind: ExprKind::TaskGet(Box::new(Expr {
                            kind: ExprKind::Local(0),
                            ty: Ty::Unit,
                            span,
                        })),
                        ty: Ty::Int(IntTy {
                            bits: 32,
                            signed: true,
                        }),
                        span,
                    }),
                ],
                value: None,
            }),
            ty: Ty::Unit,
            span,
        };
        let body = Block {
            stmts: vec![Stmt::Expr(group)],
            value: None,
        };
        let mut diagnostics = Diagnostics::new();
        validate(&body, &[], &mut diagnostics);
        assert!(
            diagnostics.has_errors(),
            "an unresolved earlier wait must not be hidden by break"
        );
    }
}
