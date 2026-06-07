use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── IDs ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkItemId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParallelGroupId(pub u64);

// ─── Enums ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkItemStatus {
    Blocked,
    Ready,
    Running,
    Suspended,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkItemKind {
    Concrete,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Data,
    Ordering,
}

// ─── Dependency edge ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Dependency {
    pub upstream: WorkItemId,
    pub kind: DepKind,
}

// ─── Work item ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WorkItem {
    pub id: WorkItemId,
    pub status: WorkItemStatus,
    pub kind: WorkItemKind,
    pub fn_ref: String,
    pub function_name: String,
    pub source_file: String,
    pub direct_args: Vec<serde_json::Value>,
    pub direct_kwargs: HashMap<String, serde_json::Value>,
    pub dag_inputs: HashMap<String, String>,
    pub timeout_seconds: u32,
    pub retries: u32,
    pub retry_backoff_seconds: f64,
    pub attempts: u32,
    pub dependencies: Vec<Dependency>,
    pub group: Option<ParallelGroupId>,
    pub submitted_by: Option<WorkItemId>,
}

// ─── Parallel group ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParallelGroup {
    pub id: ParallelGroupId,
    pub parent: WorkItemId,
    pub items: Vec<WorkItemId>,
    pub completed_count: usize,
}

// ─── Spec for adding items ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WorkItemSpec {
    pub fn_ref: String,
    pub function_name: String,
    pub source_file: String,
    pub kind: WorkItemKind,
    pub direct_args: Vec<serde_json::Value>,
    pub direct_kwargs: HashMap<String, serde_json::Value>,
    pub dag_inputs: HashMap<String, String>,
    pub timeout_seconds: u32,
    pub retries: u32,
    pub retry_backoff_seconds: f64,
}

// ─── WorkPlan ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WorkPlan {
    items: HashMap<WorkItemId, WorkItem>,
    groups: HashMap<ParallelGroupId, ParallelGroup>,
    next_item_id: u64,
    next_group_id: u64,
}

impl WorkPlan {
    // ─── Construction ──────────────────────────────────────────────────────

    /// Create an empty work plan.
    pub fn empty() -> Self {
        Self {
            items: HashMap::new(),
            groups: HashMap::new(),
            next_item_id: 0,
            next_group_id: 0,
        }
    }

    /// Add a work item with no dependencies. Status starts as Ready.
    pub fn add_item(&mut self, spec: WorkItemSpec) -> WorkItemId {
        let id = WorkItemId(self.next_item_id);
        self.next_item_id += 1;

        let item = WorkItem {
            id,
            status: WorkItemStatus::Ready,
            kind: spec.kind,
            fn_ref: spec.fn_ref,
            function_name: spec.function_name,
            source_file: spec.source_file,
            direct_args: spec.direct_args,
            direct_kwargs: spec.direct_kwargs,
            dag_inputs: spec.dag_inputs,
            timeout_seconds: spec.timeout_seconds,
            retries: spec.retries,
            retry_backoff_seconds: spec.retry_backoff_seconds,
            attempts: 0,
            dependencies: Vec::new(),
            group: None,
            submitted_by: None,
        };

        self.items.insert(id, item);
        id
    }

    /// Add a work item with dependencies. Status is Blocked or Ready depending
    /// on whether all deps are satisfied.
    pub fn add_item_with_deps(&mut self, spec: WorkItemSpec, deps: Vec<Dependency>) -> WorkItemId {
        let id = WorkItemId(self.next_item_id);
        self.next_item_id += 1;

        let status = self.compute_initial_status(&deps);

        let item = WorkItem {
            id,
            status,
            kind: spec.kind,
            fn_ref: spec.fn_ref,
            function_name: spec.function_name,
            source_file: spec.source_file,
            direct_args: spec.direct_args,
            direct_kwargs: spec.direct_kwargs,
            dag_inputs: spec.dag_inputs,
            timeout_seconds: spec.timeout_seconds,
            retries: spec.retries,
            retry_backoff_seconds: spec.retry_backoff_seconds,
            attempts: 0,
            dependencies: deps,
            group: None,
            submitted_by: None,
        };

        self.items.insert(id, item);
        id
    }

    /// Determine the initial status for an item based on its dependencies.
    fn compute_initial_status(&self, deps: &[Dependency]) -> WorkItemStatus {
        let mut all_satisfied = true;
        for dep in deps {
            let upstream_status = self.items.get(&dep.upstream).map(|i| i.status);
            match dep.kind {
                DepKind::Data => match upstream_status {
                    Some(WorkItemStatus::Done) => {}
                    Some(WorkItemStatus::Failed) | Some(WorkItemStatus::Skipped) => {
                        return WorkItemStatus::Skipped;
                    }
                    _ => {
                        all_satisfied = false;
                    }
                },
                DepKind::Ordering => match upstream_status {
                    Some(WorkItemStatus::Done)
                    | Some(WorkItemStatus::Failed)
                    | Some(WorkItemStatus::Skipped) => {}
                    _ => {
                        all_satisfied = false;
                    }
                },
            }
        }
        if all_satisfied {
            WorkItemStatus::Ready
        } else {
            WorkItemStatus::Blocked
        }
    }

    // ─── Queries ───────────────────────────────────────────────────────────

    /// Return all items with status == Ready.
    pub fn ready_items(&self) -> Vec<WorkItemId> {
        self.items
            .values()
            .filter(|item| item.status == WorkItemStatus::Ready)
            .map(|item| item.id)
            .collect()
    }

    /// Return the status of an item.
    pub fn status(&self, id: WorkItemId) -> WorkItemStatus {
        self.items[&id].status
    }

    /// Return a reference to a work item.
    pub fn item(&self, id: WorkItemId) -> &WorkItem {
        &self.items[&id]
    }

    /// Check if all items are in a terminal state (Done, Failed, or Skipped).
    pub fn is_finished(&self) -> bool {
        self.items.values().all(|item| {
            matches!(
                item.status,
                WorkItemStatus::Done | WorkItemStatus::Failed | WorkItemStatus::Skipped
            )
        })
    }

    /// Check whether two items can run concurrently (neither depends on the
    /// other transitively).
    pub fn can_run_concurrently(&self, a: WorkItemId, b: WorkItemId) -> bool {
        !self.depends_on_transitively(a, b) && !self.depends_on_transitively(b, a)
    }

    /// Check if `item` transitively depends on `target`.
    fn depends_on_transitively(&self, item: WorkItemId, target: WorkItemId) -> bool {
        let work_item = match self.items.get(&item) {
            Some(i) => i,
            None => return false,
        };

        for dep in &work_item.dependencies {
            if dep.upstream == target {
                return true;
            }
            if self.depends_on_transitively(dep.upstream, target) {
                return true;
            }
        }
        false
    }

    /// Return all items with status == Failed.
    pub fn failures(&self) -> Vec<&WorkItem> {
        self.items
            .values()
            .filter(|item| item.status == WorkItemStatus::Failed)
            .collect()
    }

    // ─── Mutations ─────────────────────────────────────────────────────────

    /// Transition an item from Ready to Running.
    pub fn mark_running(&mut self, id: WorkItemId) {
        let item = self.items.get_mut(&id).expect("WorkItem not found");
        assert_eq!(
            item.status,
            WorkItemStatus::Ready,
            "mark_running requires status == Ready, got {:?}",
            item.status
        );
        item.status = WorkItemStatus::Running;
    }

    /// Transition an item from Running to Done. Recalculate dependents and
    /// return any items that became Ready.
    pub fn mark_done(&mut self, id: WorkItemId) -> Vec<WorkItemId> {
        let item = self.items.get_mut(&id).expect("WorkItem not found");
        assert_eq!(
            item.status,
            WorkItemStatus::Running,
            "mark_done requires status == Running, got {:?}",
            item.status
        );
        item.status = WorkItemStatus::Done;
        self.propagate_completion(id)
    }

    /// Mark an item as failed. Handles retry logic and cascading.
    ///
    /// - If attempts < retries: increment attempts, set Ready, return vec![id]
    /// - If attempts >= retries: set Failed, propagate:
    ///   - Data deps downstream → Skipped (cascade)
    ///   - Ordering deps downstream → recalculate (may become Ready)
    /// - Returns all items that changed state.
    pub fn mark_failed(&mut self, id: WorkItemId) -> Vec<WorkItemId> {
        let item = self.items.get_mut(&id).expect("WorkItem not found");
        item.attempts += 1;

        if item.attempts < item.retries {
            // Retry: go back to Ready
            item.status = WorkItemStatus::Ready;
            vec![id]
        } else {
            // Permanently failed
            item.status = WorkItemStatus::Failed;
            self.propagate_failure(id)
        }
    }

    /// Transition an item from Running to Suspended.
    pub fn mark_suspended(&mut self, id: WorkItemId) {
        let item = self.items.get_mut(&id).expect("WorkItem not found");
        assert_eq!(
            item.status,
            WorkItemStatus::Running,
            "mark_suspended requires status == Running, got {:?}",
            item.status
        );
        item.status = WorkItemStatus::Suspended;
    }

    /// Transition an item from Suspended to Running.
    pub fn mark_resumed(&mut self, id: WorkItemId) {
        let item = self.items.get_mut(&id).expect("WorkItem not found");
        assert_eq!(
            item.status,
            WorkItemStatus::Suspended,
            "mark_resumed requires status == Suspended, got {:?}",
            item.status
        );
        item.status = WorkItemStatus::Running;
    }

    // ─── Dynamic expansion ─────────────────────────────────────────────────

    /// Submit parallel work items from a running parent.
    ///
    /// The parent transitions Running → Suspended. N new items are created
    /// (status = Ready, submitted_by = parent, group = new group). Returns
    /// the group id and the new item ids.
    pub fn submit_parallel(
        &mut self,
        parent: WorkItemId,
        items: Vec<WorkItemSpec>,
    ) -> (ParallelGroupId, Vec<WorkItemId>) {
        // Parent must be Running
        let parent_item = self.items.get(&parent).expect("Parent WorkItem not found");
        assert_eq!(
            parent_item.status,
            WorkItemStatus::Running,
            "submit_parallel requires parent status == Running, got {:?}",
            parent_item.status
        );

        // Suspend parent
        self.items.get_mut(&parent).unwrap().status = WorkItemStatus::Suspended;

        // Create group
        let group_id = ParallelGroupId(self.next_group_id);
        self.next_group_id += 1;

        // Create child items
        let mut child_ids = Vec::with_capacity(items.len());
        for spec in items {
            let child_id = WorkItemId(self.next_item_id);
            self.next_item_id += 1;

            let child = WorkItem {
                id: child_id,
                status: WorkItemStatus::Ready,
                kind: spec.kind,
                fn_ref: spec.fn_ref,
                function_name: spec.function_name,
                source_file: spec.source_file,
                direct_args: spec.direct_args,
                direct_kwargs: spec.direct_kwargs,
                dag_inputs: spec.dag_inputs,
                timeout_seconds: spec.timeout_seconds,
                retries: spec.retries,
                retry_backoff_seconds: spec.retry_backoff_seconds,
                attempts: 0,
                dependencies: Vec::new(),
                group: Some(group_id),
                submitted_by: Some(parent),
            };

            self.items.insert(child_id, child);
            child_ids.push(child_id);
        }

        // Create the parallel group
        let group = ParallelGroup {
            id: group_id,
            parent,
            items: child_ids.clone(),
            completed_count: 0,
        };
        self.groups.insert(group_id, group);

        (group_id, child_ids)
    }

    /// Record that one item in a parallel group has completed.
    ///
    /// If all items in the group are complete, the parent resumes (Suspended → Running)
    /// and Some(parent_id) is returned. Otherwise returns None.
    pub fn group_item_done(&mut self, group: ParallelGroupId) -> Option<WorkItemId> {
        let group_entry = self
            .groups
            .get_mut(&group)
            .expect("ParallelGroup not found");
        group_entry.completed_count += 1;

        if group_entry.completed_count == group_entry.items.len() {
            let parent_id = group_entry.parent;
            self.mark_resumed(parent_id);
            Some(parent_id)
        } else {
            None
        }
    }

    // ─── Internal helpers ──────────────────────────────────────────────────

    /// Recalculate the status of an item based on its dependencies.
    fn recalculate_status(&mut self, id: WorkItemId) {
        // Only recalculate items that are Blocked (not already running, done, etc.)
        let item = &self.items[&id];
        if !matches!(item.status, WorkItemStatus::Blocked) {
            return;
        }

        let deps: Vec<Dependency> = item.dependencies.clone();
        let mut all_satisfied = true;

        for dep in &deps {
            let upstream_status = self.items[&dep.upstream].status;
            match dep.kind {
                DepKind::Data => match upstream_status {
                    WorkItemStatus::Done => {}
                    WorkItemStatus::Failed | WorkItemStatus::Skipped => {
                        // Data dep failed/skipped → this item must be skipped
                        self.items.get_mut(&id).unwrap().status = WorkItemStatus::Skipped;
                        return;
                    }
                    _ => {
                        all_satisfied = false;
                    }
                },
                DepKind::Ordering => match upstream_status {
                    WorkItemStatus::Done | WorkItemStatus::Failed | WorkItemStatus::Skipped => {}
                    _ => {
                        all_satisfied = false;
                    }
                },
            }
        }

        if all_satisfied {
            self.items.get_mut(&id).unwrap().status = WorkItemStatus::Ready;
        }
    }

    /// After an item completes (Done), find all dependents and recalculate them.
    /// Returns items that became Ready.
    fn propagate_completion(&mut self, completed_id: WorkItemId) -> Vec<WorkItemId> {
        // Find all items that depend on completed_id
        let dependents: Vec<WorkItemId> = self
            .items
            .values()
            .filter(|item| item.dependencies.iter().any(|d| d.upstream == completed_id))
            .map(|item| item.id)
            .collect();

        let mut newly_ready = Vec::new();
        for dep_id in dependents {
            let old_status = self.items[&dep_id].status;
            self.recalculate_status(dep_id);
            let new_status = self.items[&dep_id].status;
            if old_status != WorkItemStatus::Ready && new_status == WorkItemStatus::Ready {
                newly_ready.push(dep_id);
            }
        }
        newly_ready
    }

    /// After an item permanently fails, propagate to dependents:
    /// - Data deps → Skipped (cascade further)
    /// - Ordering deps → recalculate (may become Ready)
    /// Returns all items that changed state.
    fn propagate_failure(&mut self, failed_id: WorkItemId) -> Vec<WorkItemId> {
        // Find all items that depend on failed_id
        let dependents: Vec<(WorkItemId, DepKind)> = self
            .items
            .values()
            .filter_map(|item| {
                item.dependencies
                    .iter()
                    .find(|d| d.upstream == failed_id)
                    .map(|d| (item.id, d.kind))
            })
            .collect();

        let mut changed = Vec::new();
        for (dep_id, dep_kind) in dependents {
            let item = &self.items[&dep_id];
            // Only affect blocked items
            if !matches!(item.status, WorkItemStatus::Blocked) {
                continue;
            }

            match dep_kind {
                DepKind::Data => {
                    // Skip this item and cascade
                    self.items.get_mut(&dep_id).unwrap().status = WorkItemStatus::Skipped;
                    changed.push(dep_id);
                    // Cascade: items depending on this skipped item
                    let cascaded = self.propagate_failure(dep_id);
                    changed.extend(cascaded);
                }
                DepKind::Ordering => {
                    // Recalculate — failure satisfies ordering deps
                    let old_status = self.items[&dep_id].status;
                    self.recalculate_status(dep_id);
                    let new_status = self.items[&dep_id].status;
                    if old_status != new_status {
                        changed.push(dep_id);
                    }
                }
            }
        }
        changed
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a WorkItemSpec with minimal defaults.
    fn spec(fn_ref: &str, kind: WorkItemKind) -> WorkItemSpec {
        WorkItemSpec {
            fn_ref: fn_ref.to_string(),
            function_name: fn_ref.to_string(),
            source_file: "test.py".to_string(),
            kind,
            direct_args: Vec::new(),
            direct_kwargs: HashMap::new(),
            dag_inputs: HashMap::new(),
            timeout_seconds: 60,
            retries: 0,
            retry_backoff_seconds: 1.0,
        }
    }

    #[test]
    fn test_simple_chain() {
        // A → B → C, complete in order
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item_with_deps(
            spec("B", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = plan.add_item_with_deps(
            spec("C", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: b,
                kind: DepKind::Data,
            }],
        );

        // Only A is ready
        assert_eq!(plan.status(a), WorkItemStatus::Ready);
        assert_eq!(plan.status(b), WorkItemStatus::Blocked);
        assert_eq!(plan.status(c), WorkItemStatus::Blocked);

        // Run and complete A
        plan.mark_running(a);
        let newly_ready = plan.mark_done(a);
        assert!(newly_ready.contains(&b));
        assert_eq!(plan.status(b), WorkItemStatus::Ready);

        // Run and complete B
        plan.mark_running(b);
        let newly_ready = plan.mark_done(b);
        assert!(newly_ready.contains(&c));
        assert_eq!(plan.status(c), WorkItemStatus::Ready);

        // Run and complete C
        plan.mark_running(c);
        let newly_ready = plan.mark_done(c);
        assert!(newly_ready.is_empty());

        assert!(plan.is_finished());
    }

    #[test]
    fn test_concurrent_no_deps() {
        // A and B with no deps, both Ready
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item(spec("B", WorkItemKind::Concrete));

        assert_eq!(plan.status(a), WorkItemStatus::Ready);
        assert_eq!(plan.status(b), WorkItemStatus::Ready);
        assert!(plan.can_run_concurrently(a, b));

        let ready = plan.ready_items();
        assert!(ready.contains(&a));
        assert!(ready.contains(&b));
    }

    #[test]
    fn test_fan_in() {
        // A and B → C (C blocked until both done)
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item(spec("B", WorkItemKind::Concrete));
        let c = plan.add_item_with_deps(
            spec("C", WorkItemKind::Concrete),
            vec![
                Dependency {
                    upstream: a,
                    kind: DepKind::Data,
                },
                Dependency {
                    upstream: b,
                    kind: DepKind::Data,
                },
            ],
        );

        assert_eq!(plan.status(c), WorkItemStatus::Blocked);

        // Complete A — C still blocked
        plan.mark_running(a);
        let newly_ready = plan.mark_done(a);
        assert!(!newly_ready.contains(&c));
        assert_eq!(plan.status(c), WorkItemStatus::Blocked);

        // Complete B — C becomes ready
        plan.mark_running(b);
        let newly_ready = plan.mark_done(b);
        assert!(newly_ready.contains(&c));
        assert_eq!(plan.status(c), WorkItemStatus::Ready);
    }

    #[test]
    fn test_failure_data_dep_cascades() {
        // A fails → B (data dep) skipped, C (ordering dep) ready
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item_with_deps(
            spec("B", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = plan.add_item_with_deps(
            spec("C", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Ordering,
            }],
        );

        plan.mark_running(a);
        let changed = plan.mark_failed(a);

        assert_eq!(plan.status(a), WorkItemStatus::Failed);
        assert_eq!(plan.status(b), WorkItemStatus::Skipped);
        assert_eq!(plan.status(c), WorkItemStatus::Ready);
        assert!(changed.contains(&b));
        assert!(changed.contains(&c));
    }

    #[test]
    fn test_retry_resets_to_ready() {
        // Item with retries=3 fails, goes back to Ready
        let mut plan = WorkPlan::empty();
        let mut s = spec("A", WorkItemKind::Concrete);
        s.retries = 3;
        let a = plan.add_item(s);

        plan.mark_running(a);
        let changed = plan.mark_failed(a);

        assert_eq!(plan.status(a), WorkItemStatus::Ready);
        assert_eq!(changed, vec![a]);
        assert_eq!(plan.item(a).attempts, 1);
    }

    #[test]
    fn test_retry_exhausted_marks_failed() {
        // After 3 attempts, permanently failed
        let mut plan = WorkPlan::empty();
        let mut s = spec("A", WorkItemKind::Concrete);
        s.retries = 3;
        let a = plan.add_item(s);

        // Attempt 1
        plan.mark_running(a);
        plan.mark_failed(a);
        assert_eq!(plan.status(a), WorkItemStatus::Ready);

        // Attempt 2
        plan.mark_running(a);
        plan.mark_failed(a);
        assert_eq!(plan.status(a), WorkItemStatus::Ready);

        // Attempt 3 — exhausted
        plan.mark_running(a);
        let changed = plan.mark_failed(a);
        assert_eq!(plan.status(a), WorkItemStatus::Failed);
        // No dependents, so changed is empty
        assert!(changed.is_empty());
    }

    #[test]
    fn test_submit_parallel_suspends_parent() {
        // Dynamic item submits children, becomes Suspended
        let mut plan = WorkPlan::empty();
        let parent = plan.add_item(spec("parent", WorkItemKind::Dynamic));

        plan.mark_running(parent);

        let children_specs = vec![
            spec("child_1", WorkItemKind::Concrete),
            spec("child_2", WorkItemKind::Concrete),
            spec("child_3", WorkItemKind::Concrete),
        ];

        let (group_id, child_ids) = plan.submit_parallel(parent, children_specs);

        assert_eq!(plan.status(parent), WorkItemStatus::Suspended);
        assert_eq!(child_ids.len(), 3);

        for &cid in &child_ids {
            assert_eq!(plan.status(cid), WorkItemStatus::Ready);
            assert_eq!(plan.item(cid).submitted_by, Some(parent));
            assert_eq!(plan.item(cid).group, Some(group_id));
        }
    }

    #[test]
    fn test_group_completion_resumes_parent() {
        // All children done → parent resumes
        let mut plan = WorkPlan::empty();
        let parent = plan.add_item(spec("parent", WorkItemKind::Dynamic));
        plan.mark_running(parent);

        let children_specs = vec![
            spec("child_1", WorkItemKind::Concrete),
            spec("child_2", WorkItemKind::Concrete),
        ];

        let (group_id, child_ids) = plan.submit_parallel(parent, children_specs);

        // Complete child 1
        plan.mark_running(child_ids[0]);
        plan.mark_done(child_ids[0]);
        let result = plan.group_item_done(group_id);
        assert_eq!(result, None);
        assert_eq!(plan.status(parent), WorkItemStatus::Suspended);

        // Complete child 2
        plan.mark_running(child_ids[1]);
        plan.mark_done(child_ids[1]);
        let result = plan.group_item_done(group_id);
        assert_eq!(result, Some(parent));
        assert_eq!(plan.status(parent), WorkItemStatus::Running);
    }

    #[test]
    fn test_nested_parallel() {
        // 2 levels of submit_parallel, verify ordering
        let mut plan = WorkPlan::empty();
        let root = plan.add_item(spec("root", WorkItemKind::Dynamic));
        plan.mark_running(root);

        // Root spawns level 1 children
        let l1_specs = vec![
            spec("l1_a", WorkItemKind::Dynamic),
            spec("l1_b", WorkItemKind::Concrete),
        ];
        let (root_group, l1_ids) = plan.submit_parallel(root, l1_specs);
        assert_eq!(plan.status(root), WorkItemStatus::Suspended);

        // l1_a spawns level 2 children
        plan.mark_running(l1_ids[0]);
        let l2_specs = vec![
            spec("l2_x", WorkItemKind::Concrete),
            spec("l2_y", WorkItemKind::Concrete),
        ];
        let (l1a_group, l2_ids) = plan.submit_parallel(l1_ids[0], l2_specs);
        assert_eq!(plan.status(l1_ids[0]), WorkItemStatus::Suspended);

        // Complete level 2 children
        plan.mark_running(l2_ids[0]);
        plan.mark_done(l2_ids[0]);
        assert_eq!(plan.group_item_done(l1a_group), None);

        plan.mark_running(l2_ids[1]);
        plan.mark_done(l2_ids[1]);
        let resumed = plan.group_item_done(l1a_group);
        assert_eq!(resumed, Some(l1_ids[0]));
        assert_eq!(plan.status(l1_ids[0]), WorkItemStatus::Running);

        // Complete l1_a (now resumed)
        plan.mark_done(l1_ids[0]);
        assert_eq!(plan.group_item_done(root_group), None);

        // Complete l1_b
        plan.mark_running(l1_ids[1]);
        plan.mark_done(l1_ids[1]);
        let resumed = plan.group_item_done(root_group);
        assert_eq!(resumed, Some(root));
        assert_eq!(plan.status(root), WorkItemStatus::Running);
    }

    #[test]
    fn test_is_finished_all_done() {
        // Mix of Done, Failed, Skipped = finished
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item(spec("B", WorkItemKind::Concrete));
        let c = plan.add_item_with_deps(
            spec("C", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );

        // A → Done
        plan.mark_running(a);
        plan.mark_done(a);

        // B → Failed
        plan.mark_running(b);
        plan.mark_failed(b);

        // C → Done (dep on A which is done)
        plan.mark_running(c);
        plan.mark_done(c);

        assert!(plan.is_finished());
    }

    #[test]
    fn test_is_finished_blocked_not_done() {
        // Blocked item = not finished
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let _b = plan.add_item_with_deps(
            spec("B", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );

        // Only complete A — B is still blocked (wait, it should become Ready)
        // Actually let's test with A not done
        assert!(!plan.is_finished());

        // Even after running A
        plan.mark_running(a);
        assert!(!plan.is_finished());
    }

    #[test]
    fn test_skip_cascades_through_data_deps() {
        // A fails → B skipped → C (data dep on B) also skipped
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item_with_deps(
            spec("B", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = plan.add_item_with_deps(
            spec("C", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: b,
                kind: DepKind::Data,
            }],
        );

        plan.mark_running(a);
        let changed = plan.mark_failed(a);

        assert_eq!(plan.status(a), WorkItemStatus::Failed);
        assert_eq!(plan.status(b), WorkItemStatus::Skipped);
        assert_eq!(plan.status(c), WorkItemStatus::Skipped);
        assert!(changed.contains(&b));
        assert!(changed.contains(&c));
    }

    #[test]
    fn test_ordering_dep_never_causes_skip() {
        // A fails → B (ordering) runs anyway
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item_with_deps(
            spec("B", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Ordering,
            }],
        );

        plan.mark_running(a);
        let changed = plan.mark_failed(a);

        assert_eq!(plan.status(a), WorkItemStatus::Failed);
        assert_eq!(plan.status(b), WorkItemStatus::Ready);
        assert!(changed.contains(&b));
    }

    #[test]
    fn test_graph_never_stalls_simple() {
        // Any execution order reaches finished state
        // Diamond: A → B, A → C, B → D, C → D
        let mut plan = WorkPlan::empty();
        let a = plan.add_item(spec("A", WorkItemKind::Concrete));
        let b = plan.add_item_with_deps(
            spec("B", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let c = plan.add_item_with_deps(
            spec("C", WorkItemKind::Concrete),
            vec![Dependency {
                upstream: a,
                kind: DepKind::Data,
            }],
        );
        let d = plan.add_item_with_deps(
            spec("D", WorkItemKind::Concrete),
            vec![
                Dependency {
                    upstream: b,
                    kind: DepKind::Data,
                },
                Dependency {
                    upstream: c,
                    kind: DepKind::Data,
                },
            ],
        );

        // Execute all ready items until finished
        let mut iterations = 0;
        while !plan.is_finished() {
            let ready = plan.ready_items();
            assert!(
                !ready.is_empty(),
                "Stalled! No ready items but not finished. Statuses: A={:?}, B={:?}, C={:?}, D={:?}",
                plan.status(a),
                plan.status(b),
                plan.status(c),
                plan.status(d),
            );
            for id in ready {
                plan.mark_running(id);
                plan.mark_done(id);
            }
            iterations += 1;
            assert!(iterations < 100, "Infinite loop detected");
        }

        assert_eq!(plan.status(a), WorkItemStatus::Done);
        assert_eq!(plan.status(b), WorkItemStatus::Done);
        assert_eq!(plan.status(c), WorkItemStatus::Done);
        assert_eq!(plan.status(d), WorkItemStatus::Done);
    }
}
