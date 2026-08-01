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
use crate::{expand_tagged_ty, hir_expr_diverges, TaggedType, Ty};

type Token = u32;

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
    group: Span,
    site: Span,
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
        group: Span,
        site: Span,
        kind: u8,
        incoming_generation: Token,
        incoming_epoch: Token,
    ) -> Token {
        let key = TokenKey {
            group,
            site,
            kind,
            incoming_generation,
            incoming_epoch,
        };
        if let Some(token) = self.values.get(&key) {
            return *token;
        }
        let token = self.next.saturating_add(1).max(1);
        self.next = token;
        self.values.insert(key, token);
        token
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
    id: Span,
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
    group: Span,
    epoch: Token,
    wait: Token,
    covers_through: Token,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TaskProof {
    group: Span,
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
    tokens: Tokens,
    loop_breaks: Vec<Vec<(State, Option<Proof>)>>,
    reported_gets: HashSet<Span>,
}

pub fn validate(body: &Block, tagged_types: &[TaggedType], diags: &mut Diagnostics) {
    let mut analyzer = Analyzer {
        tagged_types,
        diags,
        tokens: Tokens::default(),
        loop_breaks: Vec::new(),
        reported_gets: HashSet::new(),
    };
    let _ = analyzer.block(body, State::default(), true);
}

impl<'a> Analyzer<'a> {
    fn group<'b>(&self, state: &'b State, id: Span) -> Option<&'b Group> {
        state.groups.iter().rev().find(|group| group.id == id)
    }

    fn group_mut<'s>(&self, state: &'s mut State, id: Span) -> Option<&'s mut Group> {
        state.groups.iter_mut().rev().find(|group| group.id == id)
    }

    fn current_group(&self, state: &State) -> Option<Span> {
        state.groups.last().map(|group| group.id)
    }

    fn new_group(&mut self, id: Span) -> Group {
        let generation = self.tokens.get(id, id, INITIAL_GENERATION, 0, 0);
        let epoch = self.tokens.get(id, id, INITIAL_EPOCH, 0, 0);
        let mut valid_generations = BTreeSet::new();
        valid_generations.insert(generation);
        Group {
            id,
            generation,
            epoch,
            completed: Some(generation),
            valid_generations,
            fallible: false,
            waits: BTreeMap::new(),
            wait_order: Vec::new(),
        }
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

    fn spawn(&mut self, state: &mut State, site: Span, fallible: bool) -> Option<TaskProof> {
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
        let generation = self.tokens.get(
            id,
            site,
            SPAWN_GENERATION,
            incoming_generation,
            incoming_epoch,
        );
        let epoch = self
            .tokens
            .get(id, site, SPAWN_EPOCH, incoming_generation, incoming_epoch);
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

    fn wait(&mut self, state: &mut State, site: Span, fallible: bool) -> Option<Proof> {
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
        let wait = self.tokens.get(id, site, WAIT_TOKEN, generation, epoch);
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

    fn resolve_err(&mut self, state: &mut State, proof: WaitProof, site: Span) {
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
        let epoch = self.tokens.get(
            proof.group,
            site,
            ERR_EPOCH,
            snapshot.generation,
            proof.epoch,
        );
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

    fn join_group(&mut self, site: Span, groups: &[Group]) -> (Group, bool) {
        let Some(first) = groups.first() else {
            let generation = self.tokens.get(site, site, JOIN_GENERATION, 0, 0);
            let epoch = self.tokens.get(site, site, JOIN_EPOCH, 0, 0);
            let mut valid_generations = BTreeSet::new();
            valid_generations.insert(generation);
            return (
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
            );
        };
        if groups.iter().all(|group| group == first) {
            return (first.clone(), false);
        }
        let generation = self.tokens.get(
            first.id,
            site,
            JOIN_GENERATION,
            0,
            0,
        );
        let epoch = self.tokens.get(first.id, site, JOIN_EPOCH, 0, 0);
        let completed = groups
            .iter()
            .all(|group| group.completed == Some(group.generation));
        let mut valid_generations = BTreeSet::new();
        valid_generations.insert(generation);
        (
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
        )
    }

    fn merge_states(&mut self, site: Span, states: &[State]) -> Option<State> {
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
            let (group, differs) = self.join_group(site, &groups);
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

    fn merge_flows(&mut self, site: Span, flows: Vec<Flow>) -> Flow {
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

    fn get_error(&mut self, state: &State, proof: Option<Proof>, span: Span, report: bool) {
        if !report || self.reported_gets.contains(&span) {
            return;
        }
        let Some(Proof::Task(task)) = proof else {
            self.reported_gets.insert(span);
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
        self.reported_gets.insert(span);
        let message = self
            .group(state, task.group)
            .filter(|group| group.fallible)
            .map(|_| "cannot call '.get()' before a successful 'wait()?' — this task_group is fallible, so use 'wait()?' to join (its error propagates) before reading results")
            .unwrap_or("cannot call '.get()' before 'wait()' — a task's result is ready only after the group is joined");
        self.diags.error(message.to_string(), span);
    }

    fn block(&mut self, block: &Block, state: State, report: bool) -> Flow {
        let locals = block_local_ids(block);
        let mut current = Some(state);
        for stmt in &block.stmts {
            let Some(state) = current.take() else { break };
            current = self.stmt(stmt, state, report).state;
        }
        let flow = match (current, &block.value) {
            (Some(state), Some(value)) => self.expr(value, state, report),
            (Some(state), None) => Flow::live(state, None),
            (None, _) => Flow::dead(),
        };
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

    fn stmt(&mut self, stmt: &Stmt, state: State, report: bool) -> Flow {
        match stmt {
            Stmt::Let { local, init } => {
                let flow = self.expr(init, state, report);
                let Some(mut next) = flow.state else {
                    return Flow::dead();
                };
                next.waits.remove(local);
                next.tasks.remove(local);
                match flow.proof {
                    Some(Proof::Wait(proof)) => {
                        next.waits.insert(*local, proof);
                    }
                    Some(Proof::Task(proof)) => {
                        next.tasks.insert(*local, proof);
                    }
                    None => {}
                }
                Flow::live(next, None)
            }
            Stmt::LetTuple { init, .. } => self.expr(init, state, report).clear_proof(),
            Stmt::Assign { local, value, .. } => {
                let flow = self.expr(value, state, report);
                let Some(mut next) = flow.state else {
                    return Flow::dead();
                };
                next.waits.remove(local);
                next.tasks.remove(local);
                match flow.proof {
                    Some(Proof::Wait(proof)) => {
                        next.waits.insert(*local, proof);
                    }
                    Some(Proof::Task(proof)) => {
                        next.tasks.insert(*local, proof);
                    }
                    None => {}
                }
                Flow::live(next, None)
            }
            Stmt::AssignField { value, .. } | Stmt::AssignVecLane { value, .. } => {
                self.expr(value, state, report).clear_proof()
            }
            Stmt::AssignIndex { index, value, .. }
            | Stmt::AssignElemField { index, value, .. }
            | Stmt::AssignElem { index, value, .. } => {
                let index_flow = self.expr(index, state, report);
                let Some(index_state) = index_flow.state else {
                    return Flow::dead();
                };
                self.expr(value, index_state, report).clear_proof()
            }
            Stmt::Return(value) => {
                if let Some(value) = value {
                    let _ = self.expr(value, state, report);
                }
                Flow::dead()
            }
            Stmt::Break { value, accepted } => {
                let mut next = state;
                let mut proof = None;
                if let Some(value) = value {
                    let flow = self.expr(value, next, report);
                    let Some(after) = flow.state else {
                        return Flow::dead();
                    };
                    next = after;
                    proof = flow.proof;
                }
                if let (true, Some(breaks)) = (*accepted, self.loop_breaks.last_mut()) {
                    breaks.push((next, proof));
                }
                Flow::dead()
            }
            Stmt::Expr(expr) => self.expr(expr, state, report).clear_proof(),
        }
    }

    fn expr(&mut self, expr: &Expr, state: State, report: bool) -> Flow {
        match &expr.kind {
            ExprKind::Local(local) => Flow::live(
                state.clone(),
                state
                    .waits
                    .get(local)
                    .copied()
                    .map(Proof::Wait)
                    .or_else(|| state.tasks.get(local).copied().map(Proof::Task)),
            ),
            ExprKind::Spawn { closure, fallible } => {
                let closure_flow = self.generic_children(closure, state, report);
                let Some(mut next) = closure_flow.state else {
                    return Flow::dead();
                };
                let task = self.spawn(&mut next, expr.span, *fallible);
                Flow::live(next, task.map(Proof::Task))
            }
            ExprKind::Wait => {
                let fallible =
                    matches!(expand_tagged_ty(expr.ty, self.tagged_types), Ty::Result(..));
                let mut next = state;
                let proof = self.wait(&mut next, expr.span, fallible);
                Flow::live(next, proof)
            }
            ExprKind::TaskGet(inner) => {
                let flow = self.expr(inner, state, report);
                let Some(next) = flow.state else {
                    return Flow::dead();
                };
                self.get_error(&next, flow.proof, expr.span, report);
                Flow::live(next, None)
            }
            ExprKind::Try(inner) => {
                let flow = self.expr(inner, state, report);
                let Some(mut next) = flow.state else {
                    return Flow::dead();
                };
                if let Some(Proof::Wait(proof)) = flow.proof {
                    self.resolve_ok(&mut next, proof);
                }
                Flow::live(next, None)
            }
            ExprKind::ResultMapErr { result, f } => {
                let result_flow = self.expr(result, state, report);
                let Some(result_state) = result_flow.state else {
                    return Flow::dead();
                };
                self.expr(f, result_state, report)
                    .map_proof(result_flow.proof)
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.else_unwrap(expr.span, opt, fallback, state, report)
            }
            ExprKind::If { cond, then, els } => {
                self.if_expr(expr.span, cond, then, els, state, report)
            }
            ExprKind::Match { scrutinee, arms } => {
                self.match_expr(expr.span, scrutinee, arms, state, report)
            }
            ExprKind::Loop {
                body,
                body_locals,
                diverges,
            } => self.loop_expr(expr.span, body, body_locals, *diverges, state, report),
            ExprKind::Block(block) | ExprKind::Arena(block) | ExprKind::Unsafe(block) => {
                self.block(block, state, report)
            }
            ExprKind::TaskGroup(block) => {
                let mut nested = state;
                nested.groups.push(self.new_group(expr.span));
                let flow = self.block(block, nested, report);
                let proof = flow.proof.filter(|proof| match proof {
                    Proof::Wait(wait) => wait.group != expr.span,
                    Proof::Task(task) => task.group != expr.span,
                });
                flow.map_state(|mut state| {
                    state.waits.retain(|_, proof| proof.group != expr.span);
                    state.tasks.retain(|_, proof| proof.group != expr.span);
                    if let Some(index) =
                        state.groups.iter().rposition(|group| group.id == expr.span)
                    {
                        state.groups.remove(index);
                    }
                    state
                })
                .map_proof(proof)
            }
            ExprKind::Binary {
                op: BinOp::And | BinOp::Or,
                lhs,
                rhs,
            } => {
                let lhs_flow = self.expr(lhs, state, report);
                let Some(lhs_state) = lhs_flow.state else {
                    return Flow::dead();
                };
                let rhs_flow = self.expr(rhs, lhs_state.clone(), report);
                self.merge_flows(expr.span, vec![Flow::live(lhs_state, None), rhs_flow])
            }
            _ => self
                .generic_children(expr, state, report)
                .clear_proof()
                .map_divergence(expr),
        }
    }

    fn generic_children(&mut self, expr: &Expr, state: State, report: bool) -> Flow {
        let mut current = Some(state);
        for child in crate::direct_expr_children(expr) {
            let Some(state) = current.take() else { break };
            current = self.expr(child, state, report).state;
        }
        match current {
            Some(state) if !hir_expr_diverges(expr) => Flow::live(state, None),
            _ => Flow::dead(),
        }
    }

    fn else_unwrap(
        &mut self,
        site: Span,
        opt: &Expr,
        fallback: &Expr,
        state: State,
        report: bool,
    ) -> Flow {
        let opt_flow = self.expr(opt, state, report);
        let Some(opt_state) = opt_flow.state else {
            return Flow::dead();
        };
        let opt_ty = expand_tagged_ty(opt.ty, self.tagged_types);
        let (mut success, mut failure) = (opt_state.clone(), opt_state);
        let mut success_proof = opt_flow.proof;
        if let (Ty::Result(..), Some(Proof::Wait(proof))) = (opt_ty, opt_flow.proof) {
            self.resolve_ok(&mut success, proof);
            self.resolve_err(&mut failure, proof, site);
            success_proof = None;
        }
        let fallback_flow = self.expr(fallback, failure, report);
        self.merge_flows(
            site,
            vec![Flow::live(success, success_proof), fallback_flow],
        )
    }

    fn if_expr(
        &mut self,
        site: Span,
        cond: &Expr,
        then: &Block,
        els: &Block,
        state: State,
        report: bool,
    ) -> Flow {
        let condition = self.expr(cond, state, report);
        let Some(state) = condition.state else {
            return Flow::dead();
        };
        let then_flow = self.block(then, state.clone(), report);
        let else_flow = self.block(els, state, report);
        self.merge_flows(site, vec![then_flow, else_flow])
    }

    fn match_expr(
        &mut self,
        site: Span,
        scrutinee: &Expr,
        arms: &[MatchArm],
        state: State,
        report: bool,
    ) -> Flow {
        let scrutinee_flow = self.expr(scrutinee, state, report);
        let Some(base) = scrutinee_flow.state else {
            return Flow::dead();
        };
        let is_result = matches!(
            expand_tagged_ty(scrutinee.ty, self.tagged_types),
            Ty::Result(..)
        );
        let mut remaining = BTreeSet::from([0_u32, 1_u32]);
        let mut flows = Vec::new();
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
            flows.push(self.expr(&arm.body, arm_state, report));
        }
        if flows.is_empty() {
            Flow::live(base, None)
        } else {
            self.merge_flows(site, flows)
        }
    }

    fn loop_expr(
        &mut self,
        site: Span,
        body: &Block,
        body_locals: &Range<LocalId>,
        diverges: bool,
        entry: State,
        report: bool,
    ) -> Flow {
        let mut header = entry.clone();
        for _ in 0..64 {
            let (fallthrough, _) = self.run_loop_body(body, header.clone(), false);
            let mut predecessors = vec![entry.clone()];
            if let Some(fallthrough) = fallthrough {
                predecessors.push(fallthrough);
            }
            let next = self
                .merge_states(site, &predecessors)
                .unwrap_or_else(|| entry.clone());
            if next == header {
                break;
            }
            header = next;
        }
        let (fallthrough, mut breaks) = self.run_loop_body(body, header, report);
        let _ = fallthrough;
        if diverges || breaks.is_empty() {
            return Flow::dead();
        }
        let break_flows = breaks
            .drain(..)
            .map(|(mut state, proof)| {
                clear_locals(&mut state, body_locals);
                Flow::live(state, proof)
            })
            .collect();
        self.merge_flows(site, break_flows)
    }

    fn run_loop_body(
        &mut self,
        body: &Block,
        state: State,
        report: bool,
    ) -> (Option<State>, Vec<(State, Option<Proof>)>) {
        self.loop_breaks.push(Vec::new());
        let flow = self.block(body, state, report);
        let breaks = self.loop_breaks.pop().unwrap_or_default();
        (flow.state, breaks)
    }
}

trait FlowExt {
    fn map_proof(self, proof: Option<Proof>) -> Flow;
    fn map_state<F: FnOnce(State) -> State>(self, f: F) -> Flow;
    fn map_divergence(self, expr: &Expr) -> Flow;
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

    fn map_divergence(self, expr: &Expr) -> Flow {
        if hir_expr_diverges(expr) {
            Flow::dead()
        } else {
            self
        }
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
