//! Execution coordinator with global ready queue.
//!
//! Pure logic layer — no I/O, no processes, no sockets. Tracks items, dependencies,
//! and parallel groups. Workers are stateless — the I/O layer asks for the next
//! ready item and reports completions.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

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
    pub serializer: Option<String>,
    /// Sink outputs from stacked `@sink(...)` decorators.
    pub sinks: Vec<crate::model::SinkDecl>,
    /// Original step.inputs mapping: param_name → upstream_node_id.
    /// Used at dispatch time to resolve in-phase upstream artifacts.
    pub upstream_inputs: HashMap<String, String>,
    pub kind: String,
    pub is_dynamic: bool,
}

impl ItemSpec {
    pub fn from_step(step: &crate::planner::StreamStep) -> Self {
        Self {
            fn_ref: format!("{}:{}", step.source_file, step.function_name),
            function_name: step.function_name.to_string(),
            source_file: step.source_file.to_string(),
            direct_args: Vec::new(),
            direct_kwargs: HashMap::new(),
            dag_inputs: HashMap::new(),
            timeout_seconds: step.timeout_seconds,
            retries: step.retries,
            retry_backoff_seconds: step.retry_backoff_seconds,
            serializer: step.serializer.as_ref().map(|s| s.to_string()),
            sinks: step.sinks.clone(),
            upstream_inputs: step.inputs.clone(),
            kind: format!("{:?}", step.kind).to_lowercase(),
            is_dynamic: false,
        }
    }
}

// ─── Item ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Item {
    pub id: ItemId,
    pub step_id: crate::StepId,
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

// ─── Coordinator ──────────────────────────────────────────────────────────────

pub struct Coordinator {
    /// All items.
    items: HashMap<ItemId, Item>,
    /// Reverse dep graph: item → items that depend on it.
    dependents: HashMap<ItemId, Vec<ItemId>>,
    /// Items waiting for deps: item → set of unsatisfied upstream ids.
    pending: HashMap<ItemId, HashSet<ItemId>>,
    /// Global ready queue — items ready to execute, not yet assigned.
    ready: VecDeque<ItemId>,
    /// Items currently being executed (by some worker — we don't track which).
    executing: HashSet<ItemId>,
    /// Completed items.
    done: HashSet<ItemId>,
    /// Permanently failed items.
    failed: HashMap<ItemId, String>,
    /// Skipped items (upstream data dep failed).
    skipped: HashSet<ItemId>,
    /// Output artifacts from completed items.
    outputs: HashMap<ItemId, serde_json::Value>,
    /// Parallel groups.
    groups: HashMap<GroupId, ParallelGroup>,
    /// ID generators.
    next_item_id: u64,
    next_group_id: u64,
}

impl Coordinator {
    // ─── Construction ──────────────────────────────────────────────────────

    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            dependents: HashMap::new(),
            pending: HashMap::new(),
            ready: VecDeque::new(),
            executing: HashSet::new(),
            done: HashSet::new(),
            failed: HashMap::new(),
            skipped: HashSet::new(),
            outputs: HashMap::new(),
            groups: HashMap::new(),
            next_item_id: 0,
            next_group_id: 0,
        }
    }

    /// Add an item. If all deps satisfied, push to ready queue.
    /// If has unsatisfied deps, add to pending.
    pub fn add_item(&mut self, step_id: crate::StepId, spec: ItemSpec, deps: Vec<Dep>) -> ItemId {
        let id = ItemId(self.next_item_id);
        self.next_item_id += 1;

        let item = Item {
            id,
            step_id,
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
            let should_skip = deps.iter().any(|d| {
                d.kind == DepKind::Data
                    && (self.failed.contains_key(&d.upstream) || self.skipped.contains(&d.upstream))
            });
            if should_skip {
                self.skipped.insert(id);
            } else {
                self.ready.push_back(id);
            }
        } else {
            self.pending.insert(id, unsatisfied);
        }

        id
    }

    // ─── Scheduling ────────────────────────────────────────────────────────

    /// Pop the next ready item. Returns None if nothing is ready.
    pub fn next_ready(&mut self) -> Option<ItemId> {
        let id = self.ready.pop_front()?;
        self.executing.insert(id);
        Some(id)
    }

    /// How many items are ready to be assigned.
    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }

    // ─── Event handlers ───────────────────────────────────────────────────

    /// Mark an item as completed. Unblocks dependents into the ready queue.
    pub fn on_item_completed(&mut self, item_id: ItemId) {
        self.executing.remove(&item_id);
        self.done.insert(item_id);

        // Check if item belongs to a parallel group
        if let Some(group_id) = self.items[&item_id].group {
            let group = self.groups.get_mut(&group_id).unwrap();
            group.completed_count += 1;
        }

        // Unblock dependents
        self.unblock_dependents(item_id);
    }

    /// Mark an item as failed. Retries if budget remains, else cascades.
    pub fn on_item_failed(&mut self, item_id: ItemId, error: String) {
        self.executing.remove(&item_id);

        let item = self.items.get_mut(&item_id).unwrap();
        item.attempts += 1;

        if item.attempts < item.spec.retries {
            // Retry: push back to ready queue
            self.ready.push_back(item_id);
        } else {
            // Permanent failure
            self.failed.insert(item_id, error);

            if let Some(group_id) = self.items[&item_id].group {
                let group = self.groups.get_mut(&group_id).unwrap();
                group.completed_count += 1;
            }

            self.cascade_failure(item_id);
        }
    }

    /// Handle parallel() request. Creates a group of child items and adds them
    /// to the ready queue. Returns (group_id, child_ids).
    pub fn on_parallel_requested(
        &mut self,
        parent_id: ItemId,
        specs: Vec<ItemSpec>,
    ) -> (GroupId, Vec<ItemId>) {
        self.executing.remove(&parent_id);

        let group_id = GroupId(self.next_group_id);
        self.next_group_id += 1;

        let mut child_ids = Vec::with_capacity(specs.len());
        for spec in specs {
            let child_id = ItemId(self.next_item_id);
            self.next_item_id += 1;

            let step_id = crate::StepId {
                base: std::sync::Arc::from(spec.fn_ref.as_str()),
                partition: crate::model::PartitionKey(BTreeMap::from([(
                    "_branch".to_string(),
                    child_id.0.to_string(),
                )])),
            };
            let item = Item {
                id: child_id,
                step_id,
                spec,
                attempts: 0,
                group: Some(group_id),
                deps: Vec::new(),
            };
            self.items.insert(child_id, item);
            self.ready.push_back(child_id);
            child_ids.push(child_id);
        }

        let group = ParallelGroup {
            id: group_id,
            parent: parent_id,
            items: child_ids.clone(),
            completed_count: 0,
        };
        self.groups.insert(group_id, group);

        (group_id, child_ids)
    }

    /// Check if a parallel group is fully complete (all children done or failed).
    pub fn is_group_complete(&self, group_id: GroupId) -> bool {
        let group = &self.groups[&group_id];
        group.completed_count == group.items.len()
    }

    // ─── Phase loading ────────────────────────────────────────────────────

    /// Load all steps from a phase into the coordinator.
    /// Resolves in-phase dependencies and fills dag_inputs from provided inputs.
    /// Partitioned steps are expanded: one item per partition key.
    /// Returns the number of items added.
    pub fn load_phase(
        &mut self,
        phase: &crate::planner::Phase,
        provided: &HashMap<String, crate::dispatch::ProvidedInput>,
    ) -> usize {
        let mut count = 0;
        let mut node_to_item: HashMap<String, ItemId> = HashMap::new();
        // For partitioned steps, track all items for a given base node so
        // downstream partitioned steps can find their partition-aligned upstream.
        let mut base_to_partition_items: HashMap<
            String,
            Vec<(crate::model::PartitionKey, ItemId)>,
        > = HashMap::new();

        for stream in &phase.streams {
            let mut prev_items: Vec<ItemId> = Vec::new();
            for step in &stream.steps {
                if step.partition_keys.is_empty() {
                    // Non-partitioned step: single item
                    let step_id = step.step_id.clone();
                    let display_id = step_id.display();

                    let mut deps = Vec::new();
                    for upstream_id in step.inputs.values() {
                        if let Some(&upstream_item) = node_to_item.get(upstream_id) {
                            deps.push(Dep {
                                upstream: upstream_item,
                                kind: DepKind::Data,
                            });
                        }
                    }
                    for &p in &prev_items {
                        if !deps.iter().any(|d| d.upstream == p) {
                            deps.push(Dep {
                                upstream: p,
                                kind: DepKind::Ordering,
                            });
                        }
                    }

                    let mut spec = ItemSpec::from_step(step);
                    for (param_name, upstream_id) in &step.inputs {
                        if let Some(pi) = provided.get(upstream_id) {
                            let path = match pi {
                                crate::dispatch::ProvidedInput::Single(oref) => oref.path.clone(),
                                crate::dispatch::ProvidedInput::Collected(orefs) => {
                                    orefs.first().map(|o| o.path.clone()).unwrap_or_default()
                                }
                            };
                            spec.dag_inputs.insert(param_name.clone(), path);
                        }
                    }

                    let item_id = self.add_item(step_id, spec, deps);
                    node_to_item.insert(display_id, item_id);
                    prev_items = vec![item_id];
                    count += 1;
                } else {
                    // Partitioned step: one item per partition key
                    let base_id = step.step_id.base_id().to_string();
                    let mut partition_item_ids: Vec<ItemId> = Vec::new();

                    for pk in &step.partition_keys {
                        let partition_step_id =
                            crate::StepId::new(step.step_id.base.clone(), pk.clone());
                        let partition_display = partition_step_id.display();

                        let mut deps = Vec::new();
                        // Resolve partition-aligned upstream dependencies
                        for (param_name, upstream_id) in &step.inputs {
                            if param_name.starts_with('_') {
                                continue;
                            }
                            // Try partition-aligned first: upstream_id[pk]
                            let aligned_id = pk.display_id(upstream_id);
                            if let Some(&upstream_item) = node_to_item.get(&aligned_id) {
                                deps.push(Dep {
                                    upstream: upstream_item,
                                    kind: DepKind::Data,
                                });
                            } else if let Some(&upstream_item) = node_to_item.get(upstream_id) {
                                deps.push(Dep {
                                    upstream: upstream_item,
                                    kind: DepKind::Data,
                                });
                            }
                        }
                        // Chain ordering from previous step's partitions
                        for &p in &prev_items {
                            if !deps.iter().any(|d| d.upstream == p) {
                                deps.push(Dep {
                                    upstream: p,
                                    kind: DepKind::Ordering,
                                });
                            }
                        }

                        let mut spec = ItemSpec::from_step(step);
                        // Add partition values as direct_kwargs so they're
                        // injected into the function call.
                        for (k, v) in &pk.0 {
                            spec.direct_kwargs
                                .insert(k.clone(), serde_json::Value::String(v.clone()));
                        }
                        // Fill dag_inputs from provided (cross-phase) or
                        // rely on in-phase resolution at dispatch time via upstream_inputs.
                        for (param_name, upstream_id) in &step.inputs {
                            if param_name.starts_with('_') {
                                continue;
                            }
                            let aligned_id = pk.display_id(upstream_id);
                            if let Some(pi) = provided.get(&aligned_id) {
                                let path = match pi {
                                    crate::dispatch::ProvidedInput::Single(oref) => {
                                        oref.path.clone()
                                    }
                                    crate::dispatch::ProvidedInput::Collected(orefs) => {
                                        orefs.first().map(|o| o.path.clone()).unwrap_or_default()
                                    }
                                };
                                spec.dag_inputs.insert(param_name.clone(), path);
                            } else if let Some(pi) = provided.get(upstream_id) {
                                let path = match pi {
                                    crate::dispatch::ProvidedInput::Single(oref) => {
                                        oref.path.clone()
                                    }
                                    crate::dispatch::ProvidedInput::Collected(orefs) => {
                                        orefs.first().map(|o| o.path.clone()).unwrap_or_default()
                                    }
                                };
                                spec.dag_inputs.insert(param_name.clone(), path);
                            }
                            // Update upstream_inputs to point to partition-aligned IDs
                            // for in-phase resolution at dispatch time.
                            spec.upstream_inputs.insert(param_name.clone(), aligned_id);
                        }

                        let item_id = self.add_item(partition_step_id, spec, deps);
                        node_to_item.insert(partition_display, item_id);
                        partition_item_ids.push(item_id);
                        count += 1;
                    }

                    base_to_partition_items.insert(
                        base_id,
                        step.partition_keys
                            .iter()
                            .zip(partition_item_ids.iter())
                            .map(|(pk, &id)| (pk.clone(), id))
                            .collect(),
                    );
                    prev_items = partition_item_ids;
                }
            }
        }
        count
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

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn done_count(&self) -> usize {
        self.done.len()
    }

    pub fn failed_items(&self) -> Vec<(ItemId, &str)> {
        self.failed
            .iter()
            .map(|(id, msg)| (*id, msg.as_str()))
            .collect()
    }

    pub fn skipped_items(&self) -> Vec<ItemId> {
        self.skipped.iter().copied().collect()
    }

    pub fn is_done(&self, item_id: ItemId) -> bool {
        self.done.contains(&item_id)
    }

    /// Record an output artifact for a completed item.
    pub fn record_output(&mut self, item_id: ItemId, artifact: serde_json::Value) {
        self.outputs.insert(item_id, artifact);
    }

    /// Return all recorded outputs.
    pub fn outputs(&self) -> &HashMap<ItemId, serde_json::Value> {
        &self.outputs
    }

    // ─── Internal helpers ─────────────────────────────────────────────────

    /// Cascade a failure through data-dependency edges.
    fn cascade_failure(&mut self, item_id: ItemId) {
        let dependents = self.dependents.get(&item_id).cloned().unwrap_or_default();

        for dep_id in dependents {
            if self.done.contains(&dep_id)
                || self.failed.contains_key(&dep_id)
                || self.skipped.contains(&dep_id)
            {
                continue;
            }

            let dep_kind = self.items[&dep_id]
                .deps
                .iter()
                .find(|d| d.upstream == item_id)
                .map(|d| d.kind);

            match dep_kind {
                Some(DepKind::Data) => {
                    self.skipped.insert(dep_id);
                    self.pending.remove(&dep_id);
                    self.ready.retain(|id| *id != dep_id);
                    self.cascade_failure(dep_id);
                }
                Some(DepKind::Ordering) => {
                    if let Some(unsatisfied) = self.pending.get_mut(&dep_id) {
                        unsatisfied.remove(&item_id);
                        if unsatisfied.is_empty() {
                            self.pending.remove(&dep_id);
                            let should_skip = self.items[&dep_id].deps.iter().any(|d| {
                                d.kind == DepKind::Data
                                    && (self.failed.contains_key(&d.upstream)
                                        || self.skipped.contains(&d.upstream))
                            });
                            if should_skip {
                                self.skipped.insert(dep_id);
                                self.cascade_failure(dep_id);
                            } else {
                                self.ready.push_back(dep_id);
                            }
                        }
                    }
                }
                None => {}
            }
        }
    }

    /// Unblock dependents after successful completion.
    fn unblock_dependents(&mut self, item_id: ItemId) {
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
                    let should_skip = self.items[&dep_id].deps.iter().any(|d| {
                        d.kind == DepKind::Data
                            && (self.failed.contains_key(&d.upstream)
                                || self.skipped.contains(&d.upstream))
                    });
                    if should_skip {
                        self.skipped.insert(dep_id);
                        self.cascade_failure(dep_id);
                    } else {
                        self.ready.push_back(dep_id);
                    }
                }
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> ItemSpec {
        ItemSpec {
            fn_ref: format!("test.py:{name}"),
            function_name: name.to_string(),
            source_file: "test.py".to_string(),
            direct_args: Vec::new(),
            direct_kwargs: HashMap::new(),
            dag_inputs: HashMap::new(),
            timeout_seconds: 300,
            retries: 1,
            retry_backoff_seconds: 0.0,
            serializer: None,
            sinks: Vec::new(),
            upstream_inputs: HashMap::new(),
            kind: "asset".to_string(),
            is_dynamic: false,
        }
    }

    fn dummy_step_id(name: &str) -> crate::StepId {
        crate::StepId {
            base: std::sync::Arc::from(format!("test.py:{name}")),
            partition: crate::model::PartitionKey(std::collections::BTreeMap::new()),
        }
    }

    #[test]
    fn item_with_no_deps_goes_to_ready() {
        let mut c = Coordinator::new();
        let id = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        assert_eq!(c.ready_count(), 1);
        assert_eq!(c.next_ready(), Some(id));
        assert_eq!(c.ready_count(), 0);
    }

    #[test]
    fn item_with_deps_goes_to_pending() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let _b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        assert_eq!(c.ready_count(), 1); // only a
        assert_eq!(c.pending_count(), 1); // b is pending
    }

    #[test]
    fn completion_unblocks_dependents() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        assert_eq!(c.next_ready(), Some(a));
        c.on_item_completed(a);
        assert_eq!(c.next_ready(), Some(b));
    }

    #[test]
    fn chain_executes_in_order() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let d = c.add_item(
            dummy_step_id("c"),
            spec("c"),
            vec![Dep {
                upstream: b,
                kind: DepKind::Data,
            }],
        );

        assert_eq!(c.next_ready(), Some(a));
        c.on_item_completed(a);
        assert_eq!(c.next_ready(), Some(b));
        c.on_item_completed(b);
        assert_eq!(c.next_ready(), Some(d));
        c.on_item_completed(d);
        assert!(c.is_finished());
    }

    #[test]
    fn fan_out_all_ready_at_once() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let b = c.add_item(dummy_step_id("b"), spec("b"), vec![]);
        let d = c.add_item(dummy_step_id("c"), spec("c"), vec![]);
        assert_eq!(c.ready_count(), 3);
        // All three are ready
        let ids: HashSet<_> = [c.next_ready(), c.next_ready(), c.next_ready()]
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(ids, HashSet::from([a, b, d]));
    }

    #[test]
    fn failure_cascades_through_data_deps() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let _b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        c.next_ready(); // pop a
        c.on_item_failed(a, "boom".into());
        assert!(c.is_finished()); // b is skipped
        assert_eq!(c.skipped_items().len(), 1);
    }

    #[test]
    fn ordering_dep_satisfied_by_failure() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Ordering,
            }],
        );
        c.next_ready(); // pop a
        c.on_item_failed(a, "boom".into());
        // b should be unblocked (ordering dep satisfied by failure)
        assert_eq!(c.next_ready(), Some(b));
    }

    #[test]
    fn parallel_creates_children_in_ready_queue() {
        let mut c = Coordinator::new();
        let parent = c.add_item(dummy_step_id("parent"), spec("parent"), vec![]);
        c.next_ready(); // pop parent, mark executing

        let (group_id, children) = c.on_parallel_requested(
            parent,
            vec![spec("child_0"), spec("child_1"), spec("child_2")],
        );

        assert_eq!(children.len(), 3);
        assert_eq!(c.ready_count(), 3);
        assert!(!c.is_group_complete(group_id));
    }

    #[test]
    fn parallel_group_completes_when_all_children_done() {
        let mut c = Coordinator::new();
        let parent = c.add_item(dummy_step_id("parent"), spec("parent"), vec![]);
        c.next_ready();

        let (group_id, children) = c.on_parallel_requested(parent, vec![spec("c0"), spec("c1")]);

        let c0 = c.next_ready().unwrap();
        let c1 = c.next_ready().unwrap();
        assert_eq!(HashSet::from([c0, c1]), HashSet::from_iter(children));

        c.on_item_completed(c0);
        assert!(!c.is_group_complete(group_id));
        c.on_item_completed(c1);
        assert!(c.is_group_complete(group_id));
    }

    #[test]
    fn retry_pushes_back_to_ready() {
        let mut c = Coordinator::new();
        let mut s = spec("a");
        s.retries = 3; // 3 attempts total
        let a = c.add_item(dummy_step_id("a"), s, vec![]);
        c.next_ready();
        c.on_item_failed(a, "fail 1".into());
        // Should be back in ready (attempt 1 of 3)
        assert_eq!(c.ready_count(), 1);
        assert_eq!(c.next_ready(), Some(a));
    }

    #[test]
    fn retry_exhausted_marks_failed() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]); // retries=1 → no retry
        c.next_ready();
        c.on_item_failed(a, "boom".into());
        assert_eq!(c.failed_items().len(), 1);
        assert!(c.is_finished());
    }

    #[test]
    fn diamond_dependency() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let d = c.add_item(
            dummy_step_id("c"),
            spec("c"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let e = c.add_item(
            dummy_step_id("d"),
            spec("d"),
            vec![
                Dep {
                    upstream: b,
                    kind: DepKind::Data,
                },
                Dep {
                    upstream: d,
                    kind: DepKind::Data,
                },
            ],
        );

        // a is ready
        assert_eq!(c.next_ready(), Some(a));
        c.on_item_completed(a);
        // b and c are now ready
        assert_eq!(c.ready_count(), 2);
        let r1 = c.next_ready().unwrap();
        let r2 = c.next_ready().unwrap();
        assert_eq!(HashSet::from([r1, r2]), HashSet::from([b, d]));
        c.on_item_completed(r1);
        assert_eq!(c.ready_count(), 0); // d still waiting
        c.on_item_completed(r2);
        assert_eq!(c.next_ready(), Some(e));
        c.on_item_completed(e);
        assert!(c.is_finished());
    }

    #[test]
    fn is_finished_when_all_terminal() {
        let mut c = Coordinator::new();
        let a = c.add_item(dummy_step_id("a"), spec("a"), vec![]);
        let b = c.add_item(
            dummy_step_id("b"),
            spec("b"),
            vec![Dep {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        assert!(!c.is_finished());
        c.next_ready();
        c.on_item_completed(a);
        assert!(!c.is_finished());
        c.next_ready();
        c.on_item_completed(b);
        assert!(c.is_finished());
    }

    #[test]
    fn ten_thousand_items_no_panic() {
        let mut c = Coordinator::new();
        let mut prev = c.add_item(dummy_step_id("start"), spec("start"), vec![]);
        for i in 1..10_000 {
            prev = c.add_item(
                dummy_step_id(&format!("step_{i}")),
                spec(&format!("step_{i}")),
                vec![Dep {
                    upstream: prev,
                    kind: DepKind::Data,
                }],
            );
        }
        // Drain all
        let mut count = 0;
        loop {
            match c.next_ready() {
                Some(id) => {
                    c.on_item_completed(id);
                    count += 1;
                }
                None => {
                    if c.is_finished() {
                        break;
                    }
                }
            }
        }
        assert_eq!(count, 10_000);
    }
}
