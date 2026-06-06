//! Pure scheduling core — the functional heart of phase execution.
//!
//! This module contains **no I/O, threads, sleeps, or `Instant::now()`**. The
//! whole retry / backoff / dispatch policy is expressed as a deterministic,
//! total state machine: `Scheduler::on_event(event, now) -> Vec<Action>`. Time
//! is *injected* via `now`, so every path (retry, exhaustion, backoff promotion,
//! capacity bounding, termination) is exhaustively testable without spawning a
//! single process.
//!
//! The imperative shell (`dispatch::execute_phase`) owns the `Scheduler`,
//! interprets the returned `Action`s (spawn a worker, arm a timer, record a
//! failure), and feeds `SchedEvent`s back in. See the module docs in
//! `dispatch.rs` for the shell side.

use crate::dispatch::StepError;
use crate::model::{PartitionKey, StepId};
use crate::planner::StreamStep;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::time::{Duration, Instant};

/// A unit of work: one batch of steps to run in a single worker process.
///
/// Initial units are the planner's streams (multiple chains batched). Retry
/// units are a single failed chain's remaining sub-chain.
#[derive(Debug, Clone)]
pub struct Unit {
    pub stream_id: String,
    pub steps: Vec<StreamStep>,
    /// Predecessor output node_ids the shell must inject as provided inputs
    /// (their values live in the shell's phase-outputs map). Empty for initial
    /// units, since within-stream predecessors run in the same process and
    /// cross-phase inputs are already in the base provided map.
    pub provided_ids: Vec<String>,
    /// The base node id of the step being retried. `None` for initial units.
    /// Used by the scheduler to only charge the retry budget of the root step,
    /// not blocked descendants carried along in the same unit.
    pub retry_root: Option<String>,
}

impl Unit {
    /// Distinct base node ids produced by this unit's steps.
    fn base_ids(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for s in &self.steps {
            let b = s.step_id.base.to_string();
            if !seen.contains(&b) {
                seen.push(b);
            }
        }
        seen
    }
}

/// Input to the scheduler.
#[derive(Debug)]
pub enum SchedEvent {
    /// A worker process for `unit` exited. `failures`/`blocked` are the steps
    /// that raised / were skipped because an upstream failed. Successful steps
    /// are streamed separately (live) and don't appear here.
    UnitFinished {
        unit: Unit,
        failures: Vec<(String, StepError)>,
        blocked: Vec<String>,
    },
    /// A timer armed via `Action::SetTimer` fired (or the shell polled).
    Tick,
}

/// Effects the shell must perform. The core never performs them itself.
#[derive(Debug)]
pub enum Action {
    /// Launch one worker process for this unit.
    Spawn(Unit),
    /// Arm a timer to deliver a `Tick` at this instant (nearest backoff deadline).
    SetTimer(Instant),
    /// A step's retry budget is exhausted — record a permanent failure.
    EmitFailed(String, StepError),
    /// All work is done; the phase is complete.
    Finished,
}

/// An entry in the delay queue, ordered by deadline (earliest first). `seq`
/// breaks ties so ordering is total and stable; the `Unit` is not compared.
struct Delayed {
    deadline: Instant,
    seq: u64,
    unit: Unit,
}

impl PartialEq for Delayed {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.seq == other.seq
    }
}
impl Eq for Delayed {}
impl Ord for Delayed {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse so that `BinaryHeap` (a max-heap) yields the *earliest* deadline.
        other
            .deadline
            .cmp(&self.deadline)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}
impl PartialOrd for Delayed {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The pure scheduling state machine.
pub struct Scheduler {
    ready: VecDeque<Unit>,
    delayed: BinaryHeap<Delayed>,
    in_flight: usize,
    capacity: usize,
    /// Attempts *dispatched* per base node id (1 after the first spawn).
    attempts: HashMap<String, u32>,
    seq: u64,
    finished: bool,
}

impl Scheduler {
    pub fn new(units: Vec<Unit>, capacity: usize) -> Self {
        Scheduler {
            ready: units.into_iter().collect(),
            delayed: BinaryHeap::new(),
            in_flight: 0,
            capacity: capacity.max(1),
            attempts: HashMap::new(),
            seq: 0,
            finished: false,
        }
    }

    /// Attempts dispatched per base node id — read by the shell to persist the
    /// `attempts` count for succeeded nodes (defaults to 1 for untracked nodes).
    pub fn attempts(&self) -> &HashMap<String, u32> {
        &self.attempts
    }

    /// Kick off initial dispatch. Equivalent to a no-op event followed by a pump.
    pub fn start(&mut self, now: Instant) -> Vec<Action> {
        self.pump(now)
    }

    /// The single pure entry point: apply an event and return effects to perform.
    pub fn on_event(&mut self, event: SchedEvent, now: Instant) -> Vec<Action> {
        if self.finished {
            return Vec::new();
        }
        let mut actions = Vec::new();
        match event {
            SchedEvent::UnitFinished {
                unit,
                failures,
                blocked,
            } => {
                self.in_flight = self.in_flight.saturating_sub(1);
                self.handle_failures(&unit, failures, &blocked, now, &mut actions);
            }
            SchedEvent::Tick => {}
        }
        actions.extend(self.pump(now));
        actions
    }

    /// Decide retry-vs-exhaust for each failed step in a finished unit.
    fn handle_failures(
        &mut self,
        unit: &Unit,
        failures: Vec<(String, StepError)>,
        blocked: &[String],
        now: Instant,
        actions: &mut Vec<Action>,
    ) {
        // Group failed node_ids by their base node, preserving the error.
        let mut by_base: HashMap<String, (StepError, Vec<PartitionKey>)> = HashMap::new();
        for (node_id, err) in failures {
            let sid = StepId::parse(&node_id);
            let base = sid.base.to_string();
            let entry = by_base
                .entry(base)
                .or_insert_with(|| (err.clone(), Vec::new()));
            if !sid.partition.is_empty() {
                entry.1.push(sid.partition);
            }
        }

        for (base, (mut err, failed_pks)) in by_base {
            let step = unit.steps.iter().find(|s| s.step_id.base.as_ref() == base);
            let retries = step.map(|s| s.retries).unwrap_or(1);
            let backoff = step.map(|s| s.retry_backoff_seconds).unwrap_or(0.0);
            let tries = self.attempts.get(&base).copied().unwrap_or(1);

            if tries >= retries {
                // Exhausted — permanent failure. Report each failed node_id.
                err.attempts = tries;
                if failed_pks.is_empty() {
                    actions.push(Action::EmitFailed(base.clone(), err));
                } else {
                    for pk in &failed_pks {
                        actions.push(Action::EmitFailed(pk.display_id(&base), err.clone()));
                    }
                }
                continue;
            }

            // Schedule a retry of this base's remaining sub-chain.
            if let Some(retry_unit) = build_retry_unit(unit, &base, &failed_pks, blocked) {
                let delay = Duration::from_secs_f64((backoff * tries as f64).max(0.0));
                self.seq += 1;
                self.delayed.push(Delayed {
                    deadline: now + delay,
                    seq: self.seq,
                    unit: retry_unit,
                });
            }
        }
    }

    /// Promote due delayed units, fill capacity with spawns, arm the next timer,
    /// and detect termination. Pure given `now`.
    fn pump(&mut self, now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();
        if self.finished {
            return actions;
        }

        // Promote any delayed units whose backoff has elapsed.
        while let Some(top) = self.delayed.peek() {
            if top.deadline <= now {
                let d = self.delayed.pop().unwrap();
                self.ready.push_back(d.unit);
            } else {
                break;
            }
        }

        // Spawn ready units up to the capacity bound.
        while self.in_flight < self.capacity {
            let Some(unit) = self.ready.pop_front() else {
                break;
            };
            // For initial units, charge all bases. For retry units, only
            // charge the retry root — blocked descendants are carried along
            // but haven't consumed an attempt yet.
            if let Some(root) = &unit.retry_root {
                *self.attempts.entry(root.clone()).or_insert(0) += 1;
            } else {
                for base in unit.base_ids() {
                    *self.attempts.entry(base).or_insert(0) += 1;
                }
            }
            self.in_flight += 1;
            actions.push(Action::Spawn(unit));
        }

        // Arm a timer for the nearest still-delayed unit.
        if let Some(top) = self.delayed.peek() {
            actions.push(Action::SetTimer(top.deadline));
        }

        // Terminate iff there is nothing ready, nothing delayed, nothing running.
        if self.ready.is_empty() && self.delayed.is_empty() && self.in_flight == 0 {
            self.finished = true;
            actions.push(Action::Finished);
        }

        actions
    }
}

/// Build the retry sub-chain for a failed base: the failed step plus every
/// blocked step that transitively depends on it (within this unit). External
/// inputs (produced by succeeded steps or the base provided map) become
/// `provided_ids`.
fn build_retry_unit(
    unit: &Unit,
    failed_base: &str,
    failed_pks: &[PartitionKey],
    blocked: &[String],
) -> Option<Unit> {
    let blocked_bases: Vec<String> = blocked
        .iter()
        .map(|n| StepId::parse(n).base.to_string())
        .collect();

    // Closure: failed_base + blocked descendants reachable via the inputs graph.
    let mut included: Vec<String> = vec![failed_base.to_string()];
    loop {
        let mut grew = false;
        for step in &unit.steps {
            let base = step.step_id.base.to_string();
            if included.contains(&base) || !blocked_bases.contains(&base) {
                continue;
            }
            let depends = step
                .inputs
                .values()
                .any(|up| included.iter().any(|inc| inc == up));
            if depends {
                included.push(base);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    // Collect blocked partition keys so descendants can be restricted too.
    let blocked_pks: Vec<PartitionKey> = blocked
        .iter()
        .map(|n| StepId::parse(n).partition)
        .filter(|pk| !pk.is_empty())
        .collect();

    // Clone the included steps, restricting partition keys to only those
    // that need re-running (failed partitions for the root, blocked
    // partitions for descendants).
    let mut steps: Vec<StreamStep> = Vec::new();
    for step in &unit.steps {
        let base = step.step_id.base.to_string();
        if !included.contains(&base) {
            continue;
        }
        let mut clone = step.clone();
        if base == failed_base && !failed_pks.is_empty() {
            clone.partition_keys = failed_pks.to_vec();
        } else if base != failed_base && !blocked_pks.is_empty() {
            // Restrict descendants to only the blocked partition keys,
            // not their full original set.
            clone.partition_keys = blocked_pks.clone();
        }
        steps.push(clone);
    }
    if steps.is_empty() {
        return None;
    }

    // Inputs produced outside the sub-chain must be provided by the shell.
    let produced: Vec<String> = steps.iter().map(|s| s.step_id.base.to_string()).collect();
    let mut provided_ids: Vec<String> = Vec::new();
    for step in &steps {
        for up in step.inputs.values() {
            if !produced.contains(up) && !provided_ids.contains(up) {
                provided_ids.push(up.clone());
            }
        }
    }

    Some(Unit {
        stream_id: unit.stream_id.clone(),
        steps,
        provided_ids,
        retry_root: Some(failed_base.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeKind;
    use std::sync::Arc;

    /// Build a single-step unit for `base` with the given retry policy and inputs.
    fn mk_unit(base: &str, retries: u32, backoff: f64, inputs: &[(&str, &str)]) -> Unit {
        let inputs: HashMap<String, String> = inputs
            .iter()
            .map(|(p, u)| (p.to_string(), u.to_string()))
            .collect();
        let step = StreamStep {
            step_id: StepId::unpartitioned(base),
            kind: NodeKind::Asset,
            function_name: Arc::from("f"),
            source_file: Arc::from("f.py"),
            inputs,
            pending_partitions: HashMap::new(),
            serializer: None,
            timeout_seconds: 300,
            retries,
            retry_backoff_seconds: backoff,
            partition_keys: Vec::new(),
        };
        Unit {
            stream_id: base.to_string(),
            steps: vec![step],
            provided_ids: Vec::new(),
            retry_root: None,
        }
    }

    fn err() -> StepError {
        StepError {
            error_type: "Boom".into(),
            message: "boom".into(),
            traceback: String::new(),
            attempts: 0,
        }
    }

    #[derive(Default)]
    struct Drained {
        spawns: Vec<Unit>,
        timers: Vec<Instant>,
        failed: Vec<(String, StepError)>,
        finished: bool,
    }

    fn drain(actions: Vec<Action>) -> Drained {
        let mut d = Drained::default();
        for a in actions {
            match a {
                Action::Spawn(u) => d.spawns.push(u),
                Action::SetTimer(t) => d.timers.push(t),
                Action::EmitFailed(n, e) => d.failed.push((n, e)),
                Action::Finished => d.finished = true,
            }
        }
        d
    }

    fn finish_ok(unit: Unit) -> SchedEvent {
        SchedEvent::UnitFinished {
            unit,
            failures: Vec::new(),
            blocked: Vec::new(),
        }
    }

    fn finish_fail(unit: Unit, base: &str) -> SchedEvent {
        SchedEvent::UnitFinished {
            unit,
            failures: vec![(base.to_string(), err())],
            blocked: Vec::new(),
        }
    }

    #[test]
    fn no_per_chain_overhead_exactly_k_spawns() {
        let t0 = Instant::now();
        let units: Vec<Unit> = (0..3)
            .map(|i| mk_unit(&format!("s{i}"), 1, 0.0, &[]))
            .collect();
        let mut s = Scheduler::new(units, 3);

        let d = drain(s.start(t0));
        assert_eq!(d.spawns.len(), 3, "all 3 streams spawn at once");
        assert!(!d.finished);

        // Finish all three successfully — no further spawns, then Finished.
        let mut extra_spawns = 0;
        let mut finished = false;
        for u in d.spawns {
            let dd = drain(s.on_event(finish_ok(u), t0));
            extra_spawns += dd.spawns.len();
            finished |= dd.finished;
        }
        assert_eq!(extra_spawns, 0, "healthy streams never re-dispatch");
        assert!(finished);
    }

    #[test]
    fn capacity_bounds_concurrent_spawns() {
        let t0 = Instant::now();
        let units: Vec<Unit> = (0..4)
            .map(|i| mk_unit(&format!("s{i}"), 1, 0.0, &[]))
            .collect();
        let mut s = Scheduler::new(units, 2);

        let d = drain(s.start(t0));
        assert_eq!(d.spawns.len(), 2, "capacity caps initial spawns at 2");

        // Each completion frees exactly one slot → exactly one new spawn.
        let d2 = drain(s.on_event(finish_ok(d.spawns[0].clone()), t0));
        assert_eq!(d2.spawns.len(), 1);
        let d3 = drain(s.on_event(finish_ok(d.spawns[1].clone()), t0));
        assert_eq!(d3.spawns.len(), 1);
        // Two left in flight; finishing them drains to completion.
        drain(s.on_event(finish_ok(d2.spawns[0].clone()), t0));
        let last = drain(s.on_event(finish_ok(d3.spawns[0].clone()), t0));
        assert!(last.finished);
    }

    #[test]
    fn retry_then_succeed_uses_increasing_backoff() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(vec![mk_unit("x", 3, 1.0, &[])], 1);

        let d = drain(s.start(t0));
        let u = d.spawns.into_iter().next().unwrap();

        // 1st failure → no permanent failure, retry armed at t0 + 1*1.0s.
        let d1 = drain(s.on_event(finish_fail(u, "x"), t0));
        assert!(d1.failed.is_empty());
        assert!(d1.spawns.is_empty(), "backoff not elapsed → no spawn yet");
        assert_eq!(d1.timers, vec![t0 + Duration::from_secs(1)]);

        // Tick at the deadline → retry spawns (attempt 2).
        let d2 = drain(s.on_event(SchedEvent::Tick, t0 + Duration::from_secs(1)));
        assert_eq!(d2.spawns.len(), 1);
        let u2 = d2.spawns.into_iter().next().unwrap();

        // 2nd failure → retry armed at now + 2*1.0s (backoff grows with attempts).
        let now2 = t0 + Duration::from_secs(1);
        let d3 = drain(s.on_event(finish_fail(u2, "x"), now2));
        assert!(d3.failed.is_empty());
        assert_eq!(d3.timers, vec![now2 + Duration::from_secs(2)]);

        // Tick → attempt 3, which succeeds → Finished, never a permanent failure.
        let d4 = drain(s.on_event(SchedEvent::Tick, now2 + Duration::from_secs(2)));
        let u3 = d4.spawns.into_iter().next().unwrap();
        let d5 = drain(s.on_event(finish_ok(u3), now2 + Duration::from_secs(2)));
        assert!(d5.finished);
        assert_eq!(*s.attempts().get("x").unwrap(), 3, "3 dispatches recorded");
    }

    #[test]
    fn retry_exhausted_emits_one_permanent_failure() {
        let t0 = Instant::now();
        let mut s = Scheduler::new(vec![mk_unit("x", 2, 0.0, &[])], 1);

        let u = drain(s.start(t0)).spawns.into_iter().next().unwrap();
        // 1st failure (attempt 1 of 2). With zero backoff the retry is promoted
        // and re-spawned immediately (attempt 2), and there is no permanent failure.
        let d1 = drain(s.on_event(finish_fail(u, "x"), t0));
        assert!(d1.failed.is_empty());
        assert_eq!(d1.spawns.len(), 1, "zero-backoff retry spawns at once");
        let u2 = d1.spawns.into_iter().next().unwrap();
        // 2nd failure → budget exhausted → exactly one permanent failure.
        let d2 = drain(s.on_event(finish_fail(u2, "x"), t0));
        assert_eq!(d2.failed.len(), 1);
        assert_eq!(d2.failed[0].0, "x");
        assert_eq!(d2.failed[0].1.attempts, 2, "reports total attempts");
        assert!(d2.finished);
    }

    #[test]
    fn backoff_does_not_stall_independent_units() {
        let t0 = Instant::now();
        // One poison unit (retries=2, long backoff) + 3 healthy units; capacity 2.
        let mut units = vec![mk_unit("poison", 2, 100.0, &[])];
        units.extend((0..3).map(|i| mk_unit(&format!("h{i}"), 1, 0.0, &[])));
        let mut s = Scheduler::new(units, 2);

        // Initial: poison + h0 in flight.
        let d = drain(s.start(t0));
        assert_eq!(d.spawns.len(), 2);
        let poison = d
            .spawns
            .iter()
            .find(|u| u.stream_id == "poison")
            .unwrap()
            .clone();
        let h0 = d
            .spawns
            .iter()
            .find(|u| u.stream_id == "h0")
            .unwrap()
            .clone();

        // h0 finishes → h1 spawns (healthy work keeps flowing).
        let a = drain(s.on_event(finish_ok(h0), t0));
        assert_eq!(a.spawns.len(), 1);
        let h1 = a.spawns.into_iter().next().unwrap();

        // poison fails → goes to delay queue (NOT a permanent failure, NO slot held);
        // its freed slot immediately runs h2.
        let b = drain(s.on_event(finish_fail(poison, "poison"), t0));
        assert!(b.failed.is_empty());
        assert_eq!(
            b.spawns.len(),
            1,
            "freed slot picks up the next healthy unit"
        );
        assert_eq!(b.spawns[0].stream_id, "h2");
        let h2 = b.spawns[0].clone();
        assert!(!b.timers.is_empty(), "a backoff timer is armed for poison");

        // All healthy units finish while poison just sits in the delay queue.
        drain(s.on_event(finish_ok(h1), t0));
        let c = drain(s.on_event(finish_ok(h2), t0));
        // Nothing running, nothing ready — only the delayed poison remains.
        assert!(!c.finished, "not done: poison still backing off");
        assert!(
            !c.timers.is_empty(),
            "still waiting on the backoff deadline, holding no worker"
        );
    }

    #[test]
    fn set_timer_picks_nearest_deadline() {
        let t0 = Instant::now();
        // Two poison units with different backoffs, both fail in one capacity-2 wave.
        let s_units = vec![mk_unit("a", 2, 5.0, &[]), mk_unit("b", 2, 1.0, &[])];
        let mut s = Scheduler::new(s_units, 2);
        let d = drain(s.start(t0));
        let ua = d
            .spawns
            .iter()
            .find(|u| u.stream_id == "a")
            .unwrap()
            .clone();
        let ub = d
            .spawns
            .iter()
            .find(|u| u.stream_id == "b")
            .unwrap()
            .clone();

        drain(s.on_event(finish_fail(ua, "a"), t0)); // delayed at t0+5
        let d2 = drain(s.on_event(finish_fail(ub, "b"), t0)); // delayed at t0+1
        // Exactly one timer, at the nearest (b's) deadline.
        assert_eq!(d2.timers, vec![t0 + Duration::from_secs(1)]);
    }
}
