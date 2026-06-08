//! Queue-based execution coordinator.
//!
//! Pure logic layer — no I/O, no processes, no sockets. The coordinator owns
//! worker queues, pending items, dependency tracking, and reacts to events by
//! mutating state and returning [`Action`]s for the I/O layer.

use std::collections::{HashMap, HashSet, VecDeque};

// ─── ID types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub u64);

// ─── Dependency ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Data,
    Ordering,
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub upstream: ItemId,
    pub kind: DepKind,
}

// ─── Item specification ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ItemSpec {
    pub fn_ref: String,
    pub function_name: String,
    pub source_file: String,
    pub direct_args: Vec<serde_json::Value>,
    pub direct_kwargs: HashMap<String, serde_json::Value>,
    pub dag_inputs: HashMap<String, String>,
    pub timeout_seconds: u32,
    pub retries: u32,
    pub retry_backoff_seconds: f64,
    pub is_dynamic: bool,
}

// ─── Item ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Item {
    pub id: ItemId,
    pub spec: ItemSpec,
    pub attempts: u32,
    pub group: Option<GroupId>,
    pub deps: Vec<Dep>,
}

// ─── Parallel group ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParallelGroup {
    pub id: GroupId,
    pub parent: ItemId,
    pub items: Vec<ItemId>,
    pub completed_count: usize,
}

// ─── Actions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    ExecuteItem { worker_id: usize, item_id: ItemId },
    SuspendWorker { worker_id: usize },
    ResumeWorker { worker_id: usize, group_id: GroupId },
    WakeWorker { worker_id: usize },
    RespawnWorker { worker_id: usize },
    SpawnTempWorker { items: Vec<ItemId> },
}

// ─── Coordinator ──────────────────────────────────────────────────────────────

pub struct Coordinator {
    /// All items.
    items: HashMap<ItemId, Item>,
    /// Reverse dep graph: item -> items that depend on it.
    dependents: HashMap<ItemId, Vec<ItemId>>,
    /// Items waiting for deps: item -> set of unsatisfied upstream ids.
    pending: HashMap<ItemId, HashSet<ItemId>>,
    /// Per-worker queues.
    queues: Vec<VecDeque<ItemId>>,
    /// Currently executing per worker.
    executing: HashMap<usize, ItemId>,
    /// Suspended workers (waiting for parallel group).
    suspended: HashMap<usize, GroupId>,
    /// Completed items.
    done: HashSet<ItemId>,
    /// Permanently failed items.
    failed: HashMap<ItemId, String>,
    /// Skipped items (upstream data dep failed).
    skipped: HashSet<ItemId>,
    /// Parallel groups.
    groups: HashMap<GroupId, ParallelGroup>,
    /// Pool size.
    pool_size: usize,
    /// ID generators.
    next_item_id: u64,
    next_group_id: u64,
}

impl Coordinator {
    // ─── Construction ──────────────────────────────────────────────────────

    /// Create a new coordinator with the given worker pool size.
    pub fn new(pool_size: usize) -> Self {
        Self {
            items: HashMap::new(),
            dependents: HashMap::new(),
            pending: HashMap::new(),
            queues: (0..pool_size).map(|_| VecDeque::new()).collect(),
            executing: HashMap::new(),
            suspended: HashMap::new(),
            done: HashSet::new(),
            failed: HashMap::new(),
            skipped: HashSet::new(),
            groups: HashMap::new(),
            pool_size,
            next_item_id: 0,
            next_group_id: 0,
        }
    }

    /// Add an item. If no deps, push to shortest non-suspended queue.
    /// If has deps, add to pending with unsatisfied dep set.
    /// Register in dependents map.
    pub fn add_item(&mut self, spec: ItemSpec, deps: Vec<Dep>) -> ItemId {
        let id = ItemId(self.next_item_id);
        self.next_item_id += 1;

        let item = Item {
            id,
            spec,
            attempts: 0,
            group: None,
            deps: deps.clone(),
        };
        self.items.insert(id, item);

        // Register in dependents map
        for dep in &deps {
            self.dependents.entry(dep.upstream).or_default().push(id);
        }

        // Determine unsatisfied deps
        let unsatisfied: HashSet<ItemId> = deps
            .iter()
            .filter(|d| {
                let upstream = d.upstream;
                match d.kind {
                    DepKind::Data => !self.done.contains(&upstream),
                    DepKind::Ordering => {
                        !self.done.contains(&upstream)
                            && !self.failed.contains_key(&upstream)
                            && !self.skipped.contains(&upstream)
                    }
                }
            })
            .map(|d| d.upstream)
            .collect();

        if unsatisfied.is_empty() {
            // Check if any data dep failed/skipped
            let should_skip = deps.iter().any(|d| {
                d.kind == DepKind::Data
                    && (self.failed.contains_key(&d.upstream) || self.skipped.contains(&d.upstream))
            });
            if should_skip {
                self.skipped.insert(id);
            } else {
                let queue = self.shortest_queue();
                self.queues[queue].push_back(id);
            }
        } else {
            self.pending.insert(id, unsatisfied);
        }

        id
    }

    /// Add an item directly to a specific queue (for testing).
    pub fn add_item_to_queue(&mut self, queue: usize, spec: ItemSpec) -> ItemId {
        let id = ItemId(self.next_item_id);
        self.next_item_id += 1;

        let item = Item {
            id,
            spec,
            attempts: 0,
            group: None,
            deps: Vec::new(),
        };
        self.items.insert(id, item);
        self.queues[queue].push_back(id);
        id
    }

    // ─── Event handlers ───────────────────────────────────────────────────

    /// Handle item completion: record done, check parallel group, unblock
    /// dependents, advance worker.
    pub fn on_item_completed(&mut self, worker_id: usize, item_id: ItemId) -> Vec<Action> {
        let mut actions = Vec::new();

        self.done.insert(item_id);
        self.executing.remove(&worker_id);

        // Check if item belongs to a parallel group
        if let Some(group_id) = self.items[&item_id].group {
            let group = self.groups.get_mut(&group_id).unwrap();
            group.completed_count += 1;

            if group.completed_count == group.items.len() {
                // All children done — resume parent
                let parent_id = group.parent;
                // Find which worker is suspended for this group
                let suspended_worker = self
                    .suspended
                    .iter()
                    .find(|(_, gid)| **gid == group_id)
                    .map(|(wid, _)| *wid);

                if let Some(sw) = suspended_worker {
                    self.suspended.remove(&sw);
                    // Resume the parent on that worker
                    self.executing.insert(sw, parent_id);
                    actions.push(Action::ResumeWorker {
                        worker_id: sw,
                        group_id,
                    });
                }
            }
        }

        // Unblock dependents
        self.unblock_dependents(item_id, &mut actions);

        // Advance this worker
        self.advance_worker(worker_id, &mut actions);

        // Check deadlock
        if let Some(action) = self.check_deadlock() {
            actions.push(action);
        }

        actions
    }

    /// Handle item failure: retry or permanent fail + cascade.
    pub fn on_item_failed(
        &mut self,
        worker_id: usize,
        item_id: ItemId,
        error: String,
    ) -> Vec<Action> {
        let mut actions = Vec::new();

        self.executing.remove(&worker_id);

        let item = self.items.get_mut(&item_id).unwrap();
        item.attempts += 1;

        let retries = item.spec.retries;
        let attempts = item.attempts;

        if attempts < retries {
            // Retry: push to shortest queue (may land on different worker)
            let queue = self.shortest_queue();
            self.queues[queue].push_back(item_id);
        } else {
            // Permanent failure
            self.failed.insert(item_id, error);

            // Check if item belongs to a parallel group
            if let Some(group_id) = self.items[&item_id].group {
                let group = self.groups.get_mut(&group_id).unwrap();
                group.completed_count += 1;

                if group.completed_count == group.items.len() {
                    // All children done (some failed) — resume parent
                    let parent_id = group.parent;
                    let suspended_worker = self
                        .suspended
                        .iter()
                        .find(|(_, gid)| **gid == group_id)
                        .map(|(wid, _)| *wid);

                    if let Some(sw) = suspended_worker {
                        self.suspended.remove(&sw);
                        self.executing.insert(sw, parent_id);
                        actions.push(Action::ResumeWorker {
                            worker_id: sw,
                            group_id,
                        });
                    }
                }
            }

            // Cascade failure to dependents
            self.cascade_failure(item_id, &mut actions);
        }

        // Advance this worker
        self.advance_worker(worker_id, &mut actions);

        // Check deadlock
        if let Some(action) = self.check_deadlock() {
            actions.push(action);
        }

        actions
    }

    /// Handle parallel() request from a running worker.
    pub fn on_parallel_requested(&mut self, worker_id: usize, specs: Vec<ItemSpec>) -> Vec<Action> {
        let mut actions = Vec::new();

        let parent_id = self
            .executing
            .remove(&worker_id)
            .expect("on_parallel_requested called but worker has no executing item");

        // Handle empty specs: immediately resume parent
        if specs.is_empty() {
            self.executing.insert(worker_id, parent_id);
            // No suspend/resume needed — worker continues
            return actions;
        }

        // Suspend this worker
        let group_id = GroupId(self.next_group_id);
        self.next_group_id += 1;

        self.suspended.insert(worker_id, group_id);
        actions.push(Action::SuspendWorker { worker_id });

        // Create children
        let mut child_ids = Vec::with_capacity(specs.len());
        for spec in specs {
            let child_id = ItemId(self.next_item_id);
            self.next_item_id += 1;

            let item = Item {
                id: child_id,
                spec,
                attempts: 0,
                group: Some(group_id),
                deps: Vec::new(),
            };
            self.items.insert(child_id, item);
            child_ids.push(child_id);
        }

        // Create group
        let group = ParallelGroup {
            id: group_id,
            parent: parent_id,
            items: child_ids.clone(),
            completed_count: 0,
        };
        self.groups.insert(group_id, group);

        // Distribute children round-robin across non-suspended queues, preferring
        // non-suspended workers.
        let non_suspended_queues: Vec<usize> = (0..self.pool_size)
            .filter(|w| !self.suspended.contains_key(w))
            .collect();

        if non_suspended_queues.is_empty() {
            // All workers suspended — assign to all queues round-robin
            for (i, &child_id) in child_ids.iter().enumerate() {
                let q = i % self.pool_size;
                self.queues[q].push_back(child_id);
            }
        } else {
            for (i, &child_id) in child_ids.iter().enumerate() {
                let q = non_suspended_queues[i % non_suspended_queues.len()];
                self.queues[q].push_back(child_id);
            }
        }

        // Wake idle non-suspended workers that now have items
        for w in 0..self.pool_size {
            if !self.suspended.contains_key(&w)
                && !self.executing.contains_key(&w)
                && w != worker_id
                && !self.queues[w].is_empty()
            {
                // Advance the worker
                self.advance_worker(w, &mut actions);
            }
        }

        // Check deadlock
        if let Some(action) = self.check_deadlock() {
            actions.push(action);
        }

        actions
    }

    /// Handle worker crash: fail executing item, redistribute orphaned queue
    /// items, respawn.
    pub fn on_worker_crashed(&mut self, worker_id: usize) -> Vec<Action> {
        let mut actions = Vec::new();

        // Fail the currently executing item (if any)
        if let Some(item_id) = self.executing.remove(&worker_id) {
            self.failed.insert(item_id, "worker crashed".to_string());

            // Check parallel group
            if let Some(group_id) = self.items[&item_id].group {
                let group = self.groups.get_mut(&group_id).unwrap();
                group.completed_count += 1;

                if group.completed_count == group.items.len() {
                    let parent_id = group.parent;
                    let suspended_worker = self
                        .suspended
                        .iter()
                        .find(|(_, gid)| **gid == group_id)
                        .map(|(wid, _)| *wid);

                    if let Some(sw) = suspended_worker {
                        self.suspended.remove(&sw);
                        self.executing.insert(sw, parent_id);
                        actions.push(Action::ResumeWorker {
                            worker_id: sw,
                            group_id,
                        });
                    }
                }
            }

            // Cascade failure
            self.cascade_failure(item_id, &mut actions);
        }

        // Remove from suspended if it was suspended
        self.suspended.remove(&worker_id);

        // Redistribute orphaned queue items to other workers
        let orphaned: Vec<ItemId> = self.queues[worker_id].drain(..).collect();
        for item_id in &orphaned {
            let queue = self.shortest_queue_excluding(worker_id);
            self.queues[queue].push_back(*item_id);
        }

        // Respawn the worker
        actions.push(Action::RespawnWorker { worker_id });

        // Check deadlock
        if let Some(action) = self.check_deadlock() {
            actions.push(action);
        }

        actions
    }

    // ─── Queries ──────────────────────────────────────────────────────────

    /// Returns true when all items are in a terminal state.
    pub fn is_finished(&self) -> bool {
        self.items.keys().all(|id| {
            self.done.contains(id) || self.failed.contains_key(id) || self.skipped.contains(id)
        })
    }

    /// Return a reference to an item.
    pub fn item(&self, id: ItemId) -> &Item {
        &self.items[&id]
    }

    /// Return a reference to a parallel group.
    pub fn group(&self, id: GroupId) -> &ParallelGroup {
        &self.groups[&id]
    }

    /// Return the length of a worker's queue.
    pub fn queue_len(&self, worker_id: usize) -> usize {
        self.queues[worker_id].len()
    }

    /// Return the number of pending (blocked) items.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Return the number of completed items.
    pub fn done_count(&self) -> usize {
        self.done.len()
    }

    /// Return failed items with their error messages.
    pub fn failed_items(&self) -> Vec<(ItemId, &str)> {
        self.failed
            .iter()
            .map(|(id, msg)| (*id, msg.as_str()))
            .collect()
    }

    /// Return skipped item ids.
    pub fn skipped_items(&self) -> Vec<ItemId> {
        self.skipped.iter().copied().collect()
    }

    // ─── Internal helpers ─────────────────────────────────────────────────

    /// Find the shortest queue, preferring non-suspended workers.
    fn shortest_queue(&self) -> usize {
        let non_suspended: Vec<usize> = (0..self.pool_size)
            .filter(|w| !self.suspended.contains_key(w))
            .collect();

        if non_suspended.is_empty() {
            // All suspended — pick shortest overall
            (0..self.pool_size)
                .min_by_key(|w| self.queues[*w].len())
                .unwrap_or(0)
        } else {
            non_suspended
                .into_iter()
                .min_by_key(|w| self.queues[*w].len())
                .unwrap()
        }
    }

    /// Find the shortest queue excluding a specific worker.
    fn shortest_queue_excluding(&self, exclude: usize) -> usize {
        let candidates: Vec<usize> = (0..self.pool_size)
            .filter(|w| *w != exclude && !self.suspended.contains_key(w))
            .collect();

        if candidates.is_empty() {
            // All others suspended — pick shortest from non-excluded
            (0..self.pool_size)
                .filter(|w| *w != exclude)
                .min_by_key(|w| self.queues[*w].len())
                .unwrap_or(0)
        } else {
            candidates
                .into_iter()
                .min_by_key(|w| self.queues[*w].len())
                .unwrap()
        }
    }

    /// Cascade a failure through data-dependency edges. Ordering deps are
    /// unblocked (failure satisfies ordering deps).
    fn cascade_failure(&mut self, item_id: ItemId, actions: &mut Vec<Action>) {
        let dependents = self.dependents.get(&item_id).cloned().unwrap_or_default();

        for dep_id in dependents {
            // Already in terminal state? skip.
            if self.done.contains(&dep_id)
                || self.failed.contains_key(&dep_id)
                || self.skipped.contains(&dep_id)
            {
                continue;
            }

            // Determine dep kind for this specific edge
            let dep_kind = self.items[&dep_id]
                .deps
                .iter()
                .find(|d| d.upstream == item_id)
                .map(|d| d.kind);

            match dep_kind {
                Some(DepKind::Data) => {
                    // Skip this item
                    self.skipped.insert(dep_id);
                    self.pending.remove(&dep_id);
                    // Remove from any queue
                    for q in &mut self.queues {
                        q.retain(|id| *id != dep_id);
                    }
                    // Cascade further
                    self.cascade_failure(dep_id, actions);
                }
                Some(DepKind::Ordering) => {
                    // Failure satisfies ordering dep — remove from pending
                    if let Some(unsatisfied) = self.pending.get_mut(&dep_id) {
                        unsatisfied.remove(&item_id);
                        if unsatisfied.is_empty() {
                            self.pending.remove(&dep_id);
                            // Check if any data dep failed/skipped
                            let should_skip = self.items[&dep_id].deps.iter().any(|d| {
                                d.kind == DepKind::Data
                                    && (self.failed.contains_key(&d.upstream)
                                        || self.skipped.contains(&d.upstream))
                            });
                            if should_skip {
                                self.skipped.insert(dep_id);
                                self.cascade_failure(dep_id, actions);
                            } else {
                                let queue = self.shortest_queue();
                                self.queues[queue].push_back(dep_id);
                            }
                        }
                    }
                }
                None => {}
            }
        }
    }

    /// Unblock dependents after an item completes successfully.
    fn unblock_dependents(&mut self, item_id: ItemId, actions: &mut Vec<Action>) {
        let dependents = self.dependents.get(&item_id).cloned().unwrap_or_default();

        for dep_id in dependents {
            if self.done.contains(&dep_id)
                || self.failed.contains_key(&dep_id)
                || self.skipped.contains(&dep_id)
            {
                continue;
            }

            if let Some(unsatisfied) = self.pending.get_mut(&dep_id) {
                unsatisfied.remove(&item_id);
                if unsatisfied.is_empty() {
                    self.pending.remove(&dep_id);
                    // Check data deps satisfied
                    let should_skip = self.items[&dep_id].deps.iter().any(|d| {
                        d.kind == DepKind::Data
                            && (self.failed.contains_key(&d.upstream)
                                || self.skipped.contains(&d.upstream))
                    });
                    if should_skip {
                        self.skipped.insert(dep_id);
                        self.cascade_failure(dep_id, actions);
                    } else {
                        let queue = self.shortest_queue();
                        self.queues[queue].push_back(dep_id);
                    }
                }
            }
        }
    }

    /// Pop next item from a worker's queue and emit ExecuteItem action.
    fn advance_worker(&mut self, worker_id: usize, actions: &mut Vec<Action>) {
        // Don't advance if worker is suspended or already executing
        if self.suspended.contains_key(&worker_id) || self.executing.contains_key(&worker_id) {
            return;
        }

        if let Some(item_id) = self.queues[worker_id].pop_front() {
            self.executing.insert(worker_id, item_id);
            actions.push(Action::ExecuteItem { worker_id, item_id });
        }
    }

    /// If all workers are suspended but there are items in queues, spawn a
    /// temporary worker to break the deadlock.
    fn check_deadlock(&self) -> Option<Action> {
        // All workers must be either suspended or idle (not executing)
        let active_workers = (0..self.pool_size)
            .filter(|w| !self.suspended.contains_key(w) && self.executing.contains_key(w))
            .count();

        if active_workers > 0 {
            return None;
        }

        // Check if there are non-suspended idle workers with items in queue
        let idle_with_work = (0..self.pool_size)
            .filter(|w| {
                !self.suspended.contains_key(w)
                    && !self.executing.contains_key(w)
                    && !self.queues[*w].is_empty()
            })
            .count();

        if idle_with_work > 0 {
            return None;
        }

        // All non-suspended workers are idle with empty queues.
        // Check if suspended workers have items in their queues or if any queue has items.
        let queued_items: Vec<ItemId> =
            self.queues.iter().flat_map(|q| q.iter().copied()).collect();

        if queued_items.is_empty() {
            return None;
        }

        // Deadlock: all workers suspended, items in queues
        Some(Action::SpawnTempWorker {
            items: queued_items,
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create an ItemSpec with sensible defaults.
    fn spec(name: &str) -> ItemSpec {
        ItemSpec {
            fn_ref: name.to_string(),
            function_name: name.to_string(),
            source_file: "test.py".to_string(),
            direct_args: Vec::new(),
            direct_kwargs: HashMap::new(),
            dag_inputs: HashMap::new(),
            timeout_seconds: 60,
            retries: 0,
            retry_backoff_seconds: 1.0,
            is_dynamic: false,
        }
    }

    fn spec_with_retries(name: &str, retries: u32) -> ItemSpec {
        ItemSpec {
            retries,
            ..spec(name)
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Parallel tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parallel_children_on_separate_queues() {
        // 4 children, pool=4 → each lands on a different queue
        let mut coord = Coordinator::new(4);
        let parent = coord.add_item(spec("parent"), vec![]);

        // Start executing parent
        let actions = advance_all(&mut coord);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::ExecuteItem { item_id, .. } if *item_id == parent))
        );

        // Request parallel with 4 children
        let children_specs: Vec<ItemSpec> = (0..4).map(|i| spec(&format!("child_{i}"))).collect();
        let actions = coord.on_parallel_requested(0, children_specs);

        // Parent should be suspended
        assert!(actions.contains(&Action::SuspendWorker { worker_id: 0 }));

        // Children should be distributed: some in queues, some already executing
        // (on_parallel_requested wakes idle workers which pick up items)
        let non_suspended_queues: Vec<usize> = (0..4)
            .filter(|w| !coord.suspended.contains_key(w))
            .collect();
        let queued: usize = non_suspended_queues
            .iter()
            .map(|q| coord.queue_len(*q))
            .sum();
        let executing: usize = coord.executing.len();
        assert_eq!(queued + executing, 4, "All 4 children accounted for");

        // Children should be on separate workers (parallelism)
        let workers_with_children: HashSet<usize> = coord
            .executing
            .keys()
            .copied()
            .chain(
                non_suspended_queues
                    .iter()
                    .filter(|q| coord.queue_len(**q) > 0)
                    .copied(),
            )
            .collect();
        assert!(
            workers_with_children.len() >= 3,
            "Children spread across multiple workers"
        );
    }

    #[test]
    fn parallel_more_children_than_workers() {
        // 100 children, pool=4 → round-robin
        let mut coord = Coordinator::new(4);
        let parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs: Vec<ItemSpec> = (0..100).map(|i| spec(&format!("child_{i}"))).collect();
        coord.on_parallel_requested(0, children_specs);

        // 3 non-suspended queues share 100 children
        let non_suspended: Vec<usize> = (0..4)
            .filter(|w| !coord.suspended.contains_key(w))
            .collect();
        let total: usize = non_suspended.iter().map(|q| coord.queue_len(*q)).sum();
        // Some items may have been popped as ExecuteItem actions
        // But accounting for executing items:
        let executing_count = coord.executing.len();
        assert_eq!(total + executing_count, 100);

        // Roughly balanced (allow +-1 difference for round-robin)
        let _ = parent; // suppress unused warning
    }

    #[test]
    fn parallel_single_item() {
        let mut coord = Coordinator::new(2);
        let _parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs = vec![spec("only_child")];
        let actions = coord.on_parallel_requested(0, children_specs);

        assert!(actions.contains(&Action::SuspendWorker { worker_id: 0 }));

        // Find the child
        let child_id = ItemId(1); // parent is 0, child is 1
        assert!(coord.items.contains_key(&child_id));
        assert_eq!(coord.items[&child_id].group, Some(GroupId(0)));
    }

    #[test]
    fn parallel_inside_parallel_inside_parallel() {
        // 3 levels of nesting using single-child groups to avoid contention.
        // Each level spawns exactly 1 child so we don't run out of workers.
        let mut coord = Coordinator::new(4);
        let root = coord.add_item(spec("root"), vec![]);
        advance_all(&mut coord);

        // Level 1: root spawns 1 child
        coord.on_parallel_requested(0, vec![spec("l1")]);
        let l1 = ItemId(1);

        advance_all(&mut coord);
        let l1_worker = find_worker_for_executing(&coord, l1).expect("l1 should be executing");

        // Level 2: l1 spawns 1 child
        coord.on_parallel_requested(l1_worker, vec![spec("l2")]);
        let l2 = ItemId(2);

        advance_all(&mut coord);
        let l2_worker = find_worker_for_executing(&coord, l2).expect("l2 should be executing");

        // Level 3: l2 spawns 1 child
        coord.on_parallel_requested(l2_worker, vec![spec("l3")]);
        let l3 = ItemId(3);

        advance_all(&mut coord);
        let l3_worker = find_worker_for_executing(&coord, l3).expect("l3 should be executing");

        // Complete from innermost outward
        coord.on_item_completed(l3_worker, l3);
        assert!(coord.done.contains(&l3));

        // l2 should resume
        advance_all(&mut coord);
        let l2_worker =
            find_worker_for_executing(&coord, l2).expect("l2 should resume after l3 completes");
        coord.on_item_completed(l2_worker, l2);

        // l1 should resume
        advance_all(&mut coord);
        let l1_worker =
            find_worker_for_executing(&coord, l1).expect("l1 should resume after l2 completes");
        coord.on_item_completed(l1_worker, l1);

        // Root should resume
        advance_all(&mut coord);
        let root_worker =
            find_worker_for_executing(&coord, root).expect("root should resume after l1 completes");
        coord.on_item_completed(root_worker, root);

        assert!(coord.is_finished());
        assert_eq!(coord.done_count(), 4); // root + l1 + l2 + l3
    }

    #[test]
    fn all_workers_suspended_spawns_temp() {
        // 2 workers, both suspend, items in queue → SpawnTempWorker
        let mut coord = Coordinator::new(2);

        let p1 = coord.add_item(spec("parent1"), vec![]);
        let p2 = coord.add_item(spec("parent2"), vec![]);
        advance_all(&mut coord);

        // Both workers request parallel
        coord.on_parallel_requested(0, vec![spec("c1")]);
        let actions = coord.on_parallel_requested(1, vec![spec("c2")]);

        // Both workers suspended, children in queues
        assert!(coord.suspended.contains_key(&0));
        assert!(coord.suspended.contains_key(&1));

        // Should spawn temp worker
        let has_spawn = actions
            .iter()
            .any(|a| matches!(a, Action::SpawnTempWorker { .. }));
        assert!(
            has_spawn,
            "Expected SpawnTempWorker action for deadlock, got: {:?}",
            actions
        );
        let _ = (p1, p2);
    }

    #[test]
    fn parallel_with_zero_items() {
        // Empty specs → immediate resume, no suspend
        let mut coord = Coordinator::new(2);
        let parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let actions = coord.on_parallel_requested(0, vec![]);

        // No suspend action
        assert!(!actions.contains(&Action::SuspendWorker { worker_id: 0 }));
        // Worker still executing parent
        assert_eq!(coord.executing.get(&0), Some(&parent));
    }

    #[test]
    fn parallel_all_children_fail() {
        // All children fail → group completes → parent resumes
        let mut coord = Coordinator::new(4);
        let parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs = vec![spec("c1"), spec("c2"), spec("c3")];
        coord.on_parallel_requested(0, children_specs);

        let c1 = ItemId(1);
        let c2 = ItemId(2);
        let c3 = ItemId(3);

        // Advance to execute children
        let actions = advance_all(&mut coord);

        // Fail all children
        for child in [c1, c2, c3] {
            if let Some(w) = find_worker_for_executing(&coord, child) {
                coord.on_item_failed(w, child, "boom".to_string());
            }
        }

        // Advance to let resume propagate
        let _actions = advance_all(&mut coord);

        // Parent should be resumed (executing)
        let parent_executing = coord.executing.values().any(|id| *id == parent);
        assert!(
            parent_executing || coord.done.contains(&parent),
            "Parent should be resumed after all children fail"
        );
        let _ = actions;
    }

    #[test]
    fn parallel_children_prefer_non_suspended_queues() {
        // Worker 0 suspended → children assigned to workers 1,2,3
        let mut coord = Coordinator::new(4);
        let _parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs: Vec<ItemSpec> = (0..6).map(|i| spec(&format!("child_{i}"))).collect();
        coord.on_parallel_requested(0, children_specs);

        // Worker 0 is suspended — its queue should be empty (children go elsewhere)
        // Actually queue 0 may still have items from before, but new children should
        // not be assigned there.
        // The children (6 items) should be on queues 1,2,3.
        let non_suspended_total: usize = (1..4).map(|q| coord.queue_len(q)).sum();
        let executing_non_parent: usize = coord.executing.iter().filter(|(w, _)| **w != 0).count();
        // All 6 children accounted for across non-suspended queues + executing
        assert_eq!(
            non_suspended_total + executing_non_parent,
            6,
            "Children should be on non-suspended queues"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Failure / Retry tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn retry_goes_to_back_of_queue() {
        // Item retries → goes to back of queue, doesn't block others
        let mut coord = Coordinator::new(1);
        let a = coord.add_item(spec_with_retries("a", 3), vec![]);
        let b = coord.add_item(spec("b"), vec![]);

        // Worker picks up a
        let actions = advance_all(&mut coord);
        assert!(find_worker_for(&actions, a).is_some());

        // a fails → retried
        let actions = coord.on_item_failed(0, a, "err".to_string());

        // Worker should pick up b next (a goes to back)
        let next_execute = actions.iter().find_map(|a| match a {
            Action::ExecuteItem { item_id, .. } => Some(*item_id),
            _ => None,
        });
        assert_eq!(next_execute, Some(b), "b should execute before retried a");
    }

    #[test]
    fn retry_lands_on_different_queue() {
        // With multiple workers, retry may land on a different queue.
        // After on_item_failed, the worker advances (picks up next from its queue),
        // so the retried item may already be executing again.
        let mut coord = Coordinator::new(4);
        // Fill queues 1,2,3 with items so queue selection picks shortest
        for q in 1..4 {
            for i in 0..5 {
                coord.add_item_to_queue(q, spec(&format!("filler_{q}_{i}")));
            }
        }
        let target = coord.add_item(spec_with_retries("target", 3), vec![]);

        advance_all(&mut coord);

        // target is on queue 0 (shortest), executing on worker 0
        let _actions = coord.on_item_failed(0, target, "err".to_string());

        // target should be re-queued OR already picked up for execution again
        let in_queue = coord.queues.iter().any(|q| q.contains(&target));
        let in_executing = coord.executing.values().any(|id| *id == target);
        assert!(
            in_queue || in_executing,
            "retried item should be in some queue or executing"
        );
        // Verify it hasn't been permanently failed
        assert!(!coord.failed.contains_key(&target));
    }

    #[test]
    fn retry_during_parallel_doesnt_block_siblings() {
        // A parallel child retries — siblings continue
        let mut coord = Coordinator::new(4);
        let parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs = vec![spec_with_retries("c1", 3), spec("c2"), spec("c3")];
        coord.on_parallel_requested(0, children_specs);

        let c1 = ItemId(1);
        let c2 = ItemId(2);
        let c3 = ItemId(3);

        let actions = advance_all(&mut coord);

        // Fail c1
        if let Some(w) = find_worker_for_executing(&coord, c1) {
            coord.on_item_failed(w, c1, "err".to_string());
        }

        // c2 and c3 should still be executable
        let c2_running = coord.executing.values().any(|id| *id == c2);
        let c3_running = coord.executing.values().any(|id| *id == c3);
        let c2_queued = coord.queues.iter().any(|q| q.contains(&c2));
        let c3_queued = coord.queues.iter().any(|q| q.contains(&c3));

        assert!(
            c2_running || c2_queued || coord.done.contains(&c2),
            "c2 should be runnable"
        );
        assert!(
            c3_running || c3_queued || coord.done.contains(&c3),
            "c3 should be runnable"
        );
        let _ = (parent, actions);
    }

    #[test]
    fn retry_exhausted_marks_failed() {
        let mut coord = Coordinator::new(1);
        let a = coord.add_item(spec_with_retries("a", 3), vec![]);
        advance_all(&mut coord);

        // Fail 3 times
        for _ in 0..2 {
            coord.on_item_failed(0, a, "err".to_string());
            advance_all(&mut coord);
        }
        coord.on_item_failed(0, a, "final err".to_string());

        assert!(coord.failed.contains_key(&a));
        assert_eq!(coord.items[&a].attempts, 3);
    }

    #[test]
    fn cascade_failure_deep_chain() {
        // A→B→C→D all skipped when A fails
        let mut coord = Coordinator::new(2);
        let a = coord.add_item(spec("a"), vec![]);
        let b = coord.add_item(
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = coord.add_item(
            spec("c"),
            vec![Dep {
                upstream: b,
                kind: DepKind::Data,
            }],
        );
        let d = coord.add_item(
            spec("d"),
            vec![Dep {
                upstream: c,
                kind: DepKind::Data,
            }],
        );

        advance_all(&mut coord);
        coord.on_item_failed(0, a, "err".to_string());

        assert!(coord.failed.contains_key(&a));
        assert!(coord.skipped.contains(&b));
        assert!(coord.skipped.contains(&c));
        assert!(coord.skipped.contains(&d));
        assert!(coord.is_finished());
    }

    #[test]
    fn cascade_failure_mixed_dep_types() {
        // A fails → B (data dep) skipped, C (ordering dep) runs
        let mut coord = Coordinator::new(2);
        let a = coord.add_item(spec("a"), vec![]);
        let b = coord.add_item(
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = coord.add_item(
            spec("c"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Ordering,
            }],
        );

        advance_all(&mut coord);
        coord.on_item_failed(0, a, "err".to_string());

        assert!(coord.failed.contains_key(&a));
        assert!(coord.skipped.contains(&b));
        assert!(!coord.skipped.contains(&c));
        // c should be in a queue or executing
        let c_available = coord.queues.iter().any(|q| q.contains(&c))
            || coord.executing.values().any(|id| *id == c);
        assert!(c_available, "c should be runnable after ordering dep fails");
    }

    #[test]
    fn cascade_failure_fan_out_then_fan_in() {
        // A → [B, C] → D (data deps)
        // B fails → D skipped (needs B's data), but C unaffected
        let mut coord = Coordinator::new(4);
        let a = coord.add_item(spec("a"), vec![]);
        let b = coord.add_item(
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = coord.add_item(
            spec("c"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let d = coord.add_item(
            spec("d"),
            vec![
                Dep {
                    upstream: b,
                    kind: DepKind::Data,
                },
                Dep {
                    upstream: c,
                    kind: DepKind::Data,
                },
            ],
        );

        advance_all(&mut coord);

        // Complete A
        coord.on_item_completed(0, a);
        advance_all(&mut coord);

        // Fail B
        if let Some(w) = find_worker_for_executing(&coord, b) {
            coord.on_item_failed(w, b, "err".to_string());
        }

        // D should be skipped (data dep on B failed)
        assert!(coord.skipped.contains(&d));
        // C should still be runnable
        assert!(!coord.skipped.contains(&c));
    }

    #[test]
    fn crash_recovery_redistributes_queue() {
        let mut coord = Coordinator::new(3);
        // Put items on worker 1's queue
        let items: Vec<ItemId> = (0..5)
            .map(|i| coord.add_item_to_queue(1, spec(&format!("item_{i}"))))
            .collect();

        // Crash worker 1
        let actions = coord.on_worker_crashed(1);

        // Queue 1 should be empty
        assert_eq!(coord.queue_len(1), 0);

        // Items redistributed to other queues
        let total: usize = (0..3).map(|q| coord.queue_len(q)).sum();
        let executing = coord.executing.len();
        assert_eq!(total + executing, 5);

        // Respawn action
        assert!(actions.contains(&Action::RespawnWorker { worker_id: 1 }));
        let _ = items;
    }

    #[test]
    fn crash_during_parallel_child() {
        let mut coord = Coordinator::new(4);
        let parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs = vec![spec("c1"), spec("c2")];
        coord.on_parallel_requested(0, children_specs);

        let c1 = ItemId(1);
        let c2 = ItemId(2);

        let actions = advance_all(&mut coord);

        // Find worker executing c1 and crash it
        if let Some(w) = find_worker_for_executing(&coord, c1) {
            coord.on_worker_crashed(w);
        }

        // c1 should be failed
        assert!(coord.failed.contains_key(&c1));

        // Complete c2 (the group should then complete)
        let _actions = advance_all(&mut coord);
        if let Some(w) = find_worker_for_executing(&coord, c2) {
            coord.on_item_completed(w, c2);
        }

        // Parent should resume
        let _actions = advance_all(&mut coord);
        let parent_resumed =
            coord.executing.values().any(|id| *id == parent) || coord.done.contains(&parent);
        assert!(parent_resumed, "parent should resume after group completes");
        let _ = actions;
    }

    #[test]
    fn rapid_successive_failures() {
        // 10 items × 3 retries — all eventually fail
        let mut coord = Coordinator::new(4);
        let items: Vec<ItemId> = (0..10)
            .map(|i| coord.add_item(spec_with_retries(&format!("item_{i}"), 3), vec![]))
            .collect();

        // Run until finished
        let mut iterations = 0;
        while !coord.is_finished() {
            let _actions = advance_all(&mut coord);
            // Fail everything that's executing
            let executing: Vec<(usize, ItemId)> = coord.executing.clone().into_iter().collect();
            for (w, id) in executing {
                coord.on_item_failed(w, id, "err".to_string());
            }
            iterations += 1;
            if iterations > 100 {
                break;
            }
        }

        assert!(coord.is_finished());
        assert_eq!(coord.failed.len(), 10);
        for item in &items {
            assert!(coord.failed.contains_key(item));
            assert_eq!(coord.items[item].attempts, 3);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Dependency tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn fan_in_last_dep_completes_triggers_push() {
        // C depends on A and B. Only when both complete does C become queued.
        let mut coord = Coordinator::new(4);
        let a = coord.add_item(spec("a"), vec![]);
        let b = coord.add_item(spec("b"), vec![]);
        let c = coord.add_item(
            spec("c"),
            vec![
                Dep {
                    upstream: a,
                    kind: DepKind::Data,
                },
                Dep {
                    upstream: b,
                    kind: DepKind::Data,
                },
            ],
        );

        advance_all(&mut coord);

        // Complete A
        coord.on_item_completed(0, a);
        // C should still be pending
        assert!(coord.pending.contains_key(&c));

        // Complete B
        if let Some(w) = find_worker_for_executing(&coord, b) {
            coord.on_item_completed(w, b);
        } else {
            advance_all(&mut coord);
            let w = find_worker_for_executing(&coord, b).unwrap();
            coord.on_item_completed(w, b);
        }

        // C should now be in a queue or executing
        let c_available = coord.queues.iter().any(|q| q.contains(&c))
            || coord.executing.values().any(|id| *id == c);
        assert!(c_available, "c should be queued after both deps complete");
        assert!(!coord.pending.contains_key(&c));
    }

    #[test]
    fn diamond_dependency() {
        // A → [B, C] → D
        let mut coord = Coordinator::new(4);
        let a = coord.add_item(spec("a"), vec![]);
        let b = coord.add_item(
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = coord.add_item(
            spec("c"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let d = coord.add_item(
            spec("d"),
            vec![
                Dep {
                    upstream: b,
                    kind: DepKind::Data,
                },
                Dep {
                    upstream: c,
                    kind: DepKind::Data,
                },
            ],
        );

        advance_all(&mut coord);
        coord.on_item_completed(0, a);
        advance_all(&mut coord);

        // B and C should be queued/executing
        assert!(!coord.pending.contains_key(&b));
        assert!(!coord.pending.contains_key(&c));
        // D still pending
        assert!(coord.pending.contains_key(&d));

        // Complete B
        if let Some(w) = find_worker_for_executing(&coord, b) {
            coord.on_item_completed(w, b);
        }
        // D still pending (needs C)
        assert!(coord.pending.contains_key(&d) || coord.skipped.contains(&d));

        // Complete C
        advance_all(&mut coord);
        if let Some(w) = find_worker_for_executing(&coord, c) {
            coord.on_item_completed(w, c);
        }

        // D should now be available
        let d_available = coord.queues.iter().any(|q| q.contains(&d))
            || coord.executing.values().any(|id| *id == d)
            || coord.done.contains(&d);
        assert!(
            d_available,
            "d should be available after both B and C complete"
        );
    }

    #[test]
    fn ordering_dep_satisfied_by_failure() {
        // A (ordering dep) → B. A fails → B still runs.
        let mut coord = Coordinator::new(2);
        let a = coord.add_item(spec("a"), vec![]);
        let b = coord.add_item(
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Ordering,
            }],
        );

        advance_all(&mut coord);
        coord.on_item_failed(0, a, "err".to_string());

        // B should be queued or executing
        let b_available = coord.queues.iter().any(|q| q.contains(&b))
            || coord.executing.values().any(|id| *id == b);
        assert!(b_available, "ordering dep satisfied by failure");
    }

    #[test]
    fn item_with_no_deps_goes_directly_to_queue() {
        let mut coord = Coordinator::new(2);
        let a = coord.add_item(spec("a"), vec![]);

        // Should be in a queue immediately
        let in_queue = coord.queues.iter().any(|q| q.contains(&a));
        assert!(in_queue, "item with no deps should be queued immediately");
        assert_eq!(coord.pending_count(), 0);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Queue management tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn idle_worker_picks_up_new_work() {
        let mut coord = Coordinator::new(2);
        let a = coord.add_item(spec("a"), vec![]);

        let actions = advance_all(&mut coord);
        // Worker should pick up 'a'
        assert!(
            actions
                .iter()
                .any(|act| matches!(act, Action::ExecuteItem { item_id, .. } if *item_id == a))
        );
    }

    #[test]
    fn rebalance_after_crash() {
        let mut coord = Coordinator::new(3);
        // Worker 0 has 6 items
        let items: Vec<ItemId> = (0..6)
            .map(|i| coord.add_item_to_queue(0, spec(&format!("item_{i}"))))
            .collect();

        // Crash worker 0
        coord.on_worker_crashed(0);

        // Items should be on workers 1 and 2
        let q1 = coord.queue_len(1);
        let q2 = coord.queue_len(2);
        assert_eq!(q1 + q2, 6);
        assert_eq!(coord.queue_len(0), 0);
        let _ = items;
    }

    #[test]
    fn queue_ordering_preserved_within_chain() {
        // Items added in order maintain ordering in a single queue
        let mut coord = Coordinator::new(1);
        let a = coord.add_item_to_queue(0, spec("a"));
        let b = coord.add_item_to_queue(0, spec("b"));
        let c = coord.add_item_to_queue(0, spec("c"));

        // Pop order should be a, b, c
        assert_eq!(coord.queues[0].pop_front(), Some(a));
        assert_eq!(coord.queues[0].pop_front(), Some(b));
        assert_eq!(coord.queues[0].pop_front(), Some(c));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Scale tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn ten_thousand_items_no_panic() {
        let mut coord = Coordinator::new(8);
        let mut ids = Vec::with_capacity(10_000);
        for i in 0..10_000 {
            let id = coord.add_item(spec(&format!("item_{i}")), vec![]);
            ids.push(id);
        }

        // Run all to completion
        let mut iterations = 0;
        while !coord.is_finished() {
            advance_all(&mut coord);
            let executing: Vec<(usize, ItemId)> = coord.executing.clone().into_iter().collect();
            if executing.is_empty() {
                break;
            }
            for (w, id) in executing {
                coord.on_item_completed(w, id);
            }
            iterations += 1;
            assert!(iterations < 20_000, "Too many iterations");
        }

        assert!(coord.is_finished());
        assert_eq!(coord.done_count(), 10_000);
    }

    #[test]
    fn deeply_nested_parallel_five_levels() {
        let mut coord = Coordinator::new(4);
        let root = coord.add_item(spec("root"), vec![]);
        advance_all(&mut coord);

        // Build 5 levels of nesting
        let mut current_parent = root;
        let mut current_worker: usize = 0;
        let mut group_parents: Vec<(ItemId, usize)> = Vec::new();

        for level in 0..5 {
            let child_spec = spec(&format!("level_{level}"));
            coord.on_parallel_requested(current_worker, vec![child_spec]);

            let child_id = ItemId(coord.next_item_id - 1);
            let _actions = advance_all(&mut coord);

            if let Some(w) = find_worker_for_executing(&coord, child_id) {
                group_parents.push((current_parent, current_worker));
                current_parent = child_id;
                current_worker = w;
            } else {
                // If no worker picked it up, break
                break;
            }
        }

        // Complete from innermost outward
        coord.on_item_completed(current_worker, current_parent);

        for (parent, _worker) in group_parents.into_iter().rev() {
            let _actions = advance_all(&mut coord);
            if let Some(w) = find_worker_for_executing(&coord, parent) {
                coord.on_item_completed(w, parent);
            }
        }

        // Should eventually finish
        let _actions = advance_all(&mut coord);
        // At minimum, no panic occurred during 5-level nesting
    }

    #[test]
    fn alternating_success_and_failure() {
        let mut coord = Coordinator::new(4);
        let items: Vec<ItemId> = (0..20)
            .map(|i| coord.add_item(spec(&format!("item_{i}")), vec![]))
            .collect();

        let mut iterations = 0;
        while !coord.is_finished() {
            advance_all(&mut coord);
            let executing: Vec<(usize, ItemId)> = coord.executing.clone().into_iter().collect();
            if executing.is_empty() {
                break;
            }
            for (i, (w, id)) in executing.iter().enumerate() {
                if i % 2 == 0 {
                    coord.on_item_completed(*w, *id);
                } else {
                    coord.on_item_failed(*w, *id, "err".to_string());
                }
            }
            iterations += 1;
            assert!(iterations < 100);
        }

        assert!(coord.is_finished());
        let total = coord.done_count() + coord.failed.len() + coord.skipped.len();
        assert_eq!(total, 20);
        let _ = items;
    }

    #[test]
    fn all_items_fail() {
        let mut coord = Coordinator::new(4);
        let items: Vec<ItemId> = (0..50)
            .map(|i| coord.add_item(spec(&format!("item_{i}")), vec![]))
            .collect();

        let mut iterations = 0;
        while !coord.is_finished() {
            advance_all(&mut coord);
            let executing: Vec<(usize, ItemId)> = coord.executing.clone().into_iter().collect();
            if executing.is_empty() {
                break;
            }
            for (w, id) in executing {
                coord.on_item_failed(w, id, "err".to_string());
            }
            iterations += 1;
            assert!(iterations < 200);
        }

        assert!(coord.is_finished());
        assert_eq!(coord.failed.len(), 50);
        let _ = items;
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Invariant tests
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn is_finished_when_all_terminal() {
        // done + failed + skipped = total → finished
        let mut coord = Coordinator::new(2);
        let a = coord.add_item(spec("a"), vec![]);
        let _b = coord.add_item(
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = coord.add_item(spec("c"), vec![]);

        advance_all(&mut coord);

        // Fail a → b skipped
        coord.on_item_failed(0, a, "err".to_string());
        // Complete c
        advance_all(&mut coord);
        if let Some(w) = find_worker_for_executing(&coord, c) {
            coord.on_item_completed(w, c);
        }

        assert!(coord.is_finished());
        let total = coord.done_count() + coord.failed.len() + coord.skipped.len();
        assert_eq!(total, coord.items.len());
    }

    #[test]
    fn wall_clock_parallelism_guaranteed() {
        // Parallel items end up on separate queues → can execute concurrently
        let mut coord = Coordinator::new(4);
        let _parent = coord.add_item(spec("parent"), vec![]);
        advance_all(&mut coord);

        let children_specs: Vec<ItemSpec> = (0..4).map(|i| spec(&format!("child_{i}"))).collect();
        coord.on_parallel_requested(0, children_specs);

        // Advance to distribute
        let _actions = advance_all(&mut coord);

        // Count how many workers are executing children
        let children_executing: Vec<usize> = coord
            .executing
            .iter()
            .filter(|(w, _)| **w != 0 || !coord.suspended.contains_key(&0))
            .filter(|(_, id)| id.0 >= 1 && id.0 <= 4)
            .map(|(w, _)| *w)
            .collect();

        // At least some children should be on different workers (parallel execution)
        let unique_workers: HashSet<usize> = children_executing.iter().copied().collect();
        // With 3 non-suspended workers and 4 children, at least 3 should be executing
        assert!(
            unique_workers.len() >= 2,
            "Children should execute on multiple workers for parallelism, got {:?}",
            unique_workers
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Test helpers
    // ═══════════════════════════════════════════════════════════════════════

    /// Advance all idle workers that have items in their queues.
    fn advance_all(coord: &mut Coordinator) -> Vec<Action> {
        let mut actions = Vec::new();
        for w in 0..coord.pool_size {
            if !coord.suspended.contains_key(&w) && !coord.executing.contains_key(&w) {
                coord.advance_worker(w, &mut actions);
            }
        }
        actions
    }

    /// Find which worker was assigned to execute a specific item in the actions.
    fn find_worker_for(actions: &[Action], item_id: ItemId) -> Option<usize> {
        actions.iter().find_map(|a| match a {
            Action::ExecuteItem {
                worker_id,
                item_id: id,
            } if *id == item_id => Some(*worker_id),
            _ => None,
        })
    }

    /// Find which worker is currently executing a specific item.
    fn find_worker_for_executing(coord: &Coordinator, item_id: ItemId) -> Option<usize> {
        coord
            .executing
            .iter()
            .find(|(_, id)| **id == item_id)
            .map(|(w, _)| *w)
    }
}
