//! Executor — maps a pure `WorkPlan` to OS processes via Unix sockets.
//!
//! The executor runs a single-threaded polling loop:
//! 1. Check WorkPlan for ready items
//! 2. Batch ready items, spawn workers (up to pool_size active)
//! 3. Accept socket connections from workers
//! 4. Process messages from connected workers
//! 5. Update the WorkPlan based on events
//! 6. Repeat until plan.is_finished()
//!
//! Each worker gets a unique Unix socket path. The executor spawns the Python
//! worker with `BARCA_SOCKET=<path>` env var. The worker connects to the socket
//! on startup.

use std::collections::HashMap;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::protocol::{self, CoordinatorMessage, ParallelResult, SubmitItem, WorkerMessage};
use crate::work_plan::*;

// ─── Active and Suspended Workers ────────────────────────────────────────────

/// A running worker process and its socket connection.
struct ActiveWorker {
    /// The OS child process.
    child: Child,
    /// Socket stream for communication.
    stream: UnixStream,
    /// The work items assigned to this worker.
    items: Vec<WorkItemId>,
    /// Socket path (for cleanup).
    socket_path: PathBuf,
    /// When this worker was spawned.
    #[allow(dead_code)]
    started_at: Instant,
}

/// A suspended worker (waiting for parallel response).
struct SuspendedWorker {
    child: Child,
    stream: UnixStream,
    items: Vec<WorkItemId>,
    socket_path: PathBuf,
    #[allow(dead_code)]
    group: ParallelGroupId,
}

// ─── Configuration ───────────────────────────────────────────────────────────

/// Configuration for the executor.
pub struct ExecutorConfig {
    /// Maximum number of concurrent worker processes.
    pub pool_size: usize,
    /// Path to the Python interpreter.
    pub python: PathBuf,
    /// Unique run identifier (used for socket paths).
    pub run_id: String,
    /// Max items to batch into one worker process.
    pub batch_size: usize,
}

// ─── Executor ────────────────────────────────────────────────────────────────

/// The executor — maps WorkPlan to processes.
pub struct Executor {
    config: ExecutorConfig,
    plan: WorkPlan,
    active: HashMap<WorkItemId, ActiveWorker>,
    suspended: HashMap<WorkItemId, SuspendedWorker>,
    /// Results collected from completed parallel branches (group -> results by position).
    parallel_results: HashMap<ParallelGroupId, Vec<Option<serde_json::Value>>>,
    next_worker_id: u64,
}

/// Result of running the executor.
pub struct ExecutorResult {
    /// Function references of all permanently-failed items.
    pub failures: Vec<String>,
}

impl Executor {
    /// Create a new executor with the given configuration and work plan.
    pub fn new(config: ExecutorConfig, plan: WorkPlan) -> Self {
        Self {
            config,
            plan,
            active: HashMap::new(),
            suspended: HashMap::new(),
            parallel_results: HashMap::new(),
            next_worker_id: 0,
        }
    }

    /// Run the executor to completion. Returns when all items are Done/Failed/Skipped.
    /// This is the main entry point.
    pub fn run(&mut self) -> ExecutorResult {
        loop {
            // 1. Spawn workers for ready items (up to capacity)
            self.spawn_ready_workers();

            // 2. Poll all active workers for messages (non-blocking with timeout)
            self.poll_workers();

            // 3. Check if done
            if self.plan.is_finished() && self.active.is_empty() {
                break;
            }

            // Small sleep to avoid busy-wait (1ms)
            std::thread::sleep(Duration::from_millis(1));
        }

        ExecutorResult {
            failures: self
                .plan
                .failures()
                .into_iter()
                .map(|w| w.fn_ref.clone())
                .collect(),
        }
    }

    fn can_spawn(&self) -> bool {
        self.active.len() < self.config.pool_size
    }

    /// Spawn workers for ready items, batching them.
    fn spawn_ready_workers(&mut self) {
        while self.can_spawn() {
            let ready = self.plan.ready_items();
            if ready.is_empty() {
                break;
            }

            // Take up to batch_size items for one worker
            let batch: Vec<WorkItemId> = ready.into_iter().take(self.config.batch_size).collect();

            // Mark all as running
            for &id in &batch {
                self.plan.mark_running(id);
            }

            // Spawn the worker
            match self.spawn_worker(&batch) {
                Ok(worker) => {
                    // Use the first item as the key (worker handles a batch)
                    let key = batch[0];
                    self.active.insert(key, worker);
                }
                Err(e) => {
                    // Spawn failed — mark all items as failed
                    eprintln!("[barca] spawn error: {e}");
                    for &id in &batch {
                        self.plan.mark_failed(id);
                    }
                }
            }
        }
    }

    /// Spawn a single worker process for a batch of items.
    fn spawn_worker(&mut self, items: &[WorkItemId]) -> Result<ActiveWorker, String> {
        let worker_id = self.next_worker_id;
        self.next_worker_id += 1;

        let sock_path = protocol::socket_path(&self.config.run_id, &format!("w{worker_id}"));

        // Create the Unix listener BEFORE spawning the worker
        let listener =
            UnixListener::bind(&sock_path).map_err(|e| format!("socket bind failed: {e}"))?;

        // Build the batch JSON for this worker
        let batch_json = self.build_batch_json(items);
        let batch_file = tempfile::NamedTempFile::new().map_err(|e| format!("temp file: {e}"))?;
        std::fs::write(batch_file.path(), &batch_json).map_err(|e| format!("write batch: {e}"))?;
        let batch_path = batch_file.into_temp_path();

        // Spawn the worker
        let child = Command::new(&self.config.python)
            .args(["-m", "barca._worker"])
            .arg(batch_path.as_os_str())
            .env("BARCA_SOCKET", sock_path.to_str().unwrap_or(""))
            .env("BARCA_WORKER", "1")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn failed: {e}"))?;

        // Accept the connection (blocking — worker should connect promptly)
        listener
            .set_nonblocking(false)
            .map_err(|e| format!("set_nonblocking: {e}"))?;
        let (stream, _) = listener
            .accept()
            .map_err(|e| format!("socket accept failed: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok();

        // Clean up the listener (we only need one connection per worker)
        drop(listener);
        // Remove the batch temp file (worker has already read it by the time it connects)
        drop(batch_path);

        Ok(ActiveWorker {
            child,
            stream,
            items: items.to_vec(),
            socket_path: sock_path,
            started_at: Instant::now(),
        })
    }

    /// Build batch JSON for items (same format as existing worker protocol).
    fn build_batch_json(&self, items: &[WorkItemId]) -> String {
        let steps: Vec<serde_json::Value> = items
            .iter()
            .map(|&id| {
                let item = self.plan.item(id);
                serde_json::json!({
                    "node_id": item.fn_ref,
                    "function_name": item.function_name,
                    "source_file": item.source_file,
                    "kind": "task",
                    "inputs": item.dag_inputs,
                    "timeout_seconds": item.timeout_seconds,
                    "direct_args": item.direct_args,
                    "direct_kwargs": item.direct_kwargs,
                    "serializer": "json",
                })
            })
            .collect();

        serde_json::json!({
            "stream_id": format!("executor-{}", self.next_worker_id),
            "artifact_dir": ".barca/artifacts",
            "steps": steps,
            "provided_inputs": {},
        })
        .to_string()
    }

    /// Poll all active workers for messages.
    fn poll_workers(&mut self) {
        let keys: Vec<WorkItemId> = self.active.keys().copied().collect();
        for key in keys {
            // Check if the worker is still in active (may have been moved during iteration)
            if !self.active.contains_key(&key) {
                continue;
            }

            let worker = self.active.get_mut(&key).unwrap();

            // Try to read a message (non-blocking due to read timeout)
            match protocol::read_message::<_, WorkerMessage>(&mut worker.stream) {
                Ok(Some(msg)) => {
                    self.handle_worker_message(key, msg);
                }
                Ok(None) => {
                    // EOF — worker disconnected
                    self.handle_worker_disconnect(key);
                }
                Err(ref e) if is_would_block_or_timeout(e) => {
                    // No message yet, that's fine
                }
                Err(_) => {
                    // Read error — treat as crash
                    self.handle_worker_disconnect(key);
                }
            }
        }
    }

    fn handle_worker_message(&mut self, key: WorkItemId, msg: WorkerMessage) {
        match msg {
            WorkerMessage::StepCompleted { node_id, .. } => {
                if let Some(id) = self.find_item_by_fn_ref(&node_id) {
                    self.plan.mark_done(id);
                    // Check if this item is in a parallel group
                    if let Some(group) = self.plan.item(id).group {
                        if let Some(parent) = self.plan.group_item_done(group) {
                            self.resume_parent(parent, group);
                        }
                    }
                }
            }
            WorkerMessage::StepError {
                node_id, message, ..
            } => {
                eprintln!("[barca] step error in {node_id}: {message}");
                if let Some(id) = self.find_item_by_fn_ref(&node_id) {
                    self.plan.mark_failed(id);
                    if let Some(group) = self.plan.item(id).group {
                        self.plan.group_item_done(group);
                    }
                }
            }
            WorkerMessage::Submit { items } => {
                self.handle_submit(key, items);
            }
            WorkerMessage::Heartbeat => {
                // Worker is alive — nothing to do for now
            }
            WorkerMessage::Blocked { node_id, reason } => {
                eprintln!("[barca] step blocked {node_id}: {reason}");
                if let Some(id) = self.find_item_by_fn_ref(&node_id) {
                    self.plan.mark_failed(id);
                }
            }
        }
    }

    fn handle_submit(&mut self, parent_key: WorkItemId, items: Vec<SubmitItem>) {
        // Convert SubmitItems to WorkItemSpecs
        let specs: Vec<WorkItemSpec> = items
            .iter()
            .map(|item| {
                let (source_file, function_name) = item
                    .fn_ref
                    .rsplit_once(':')
                    .map(|(f, n)| (f.to_string(), n.to_string()))
                    .unwrap_or_else(|| (String::new(), item.fn_ref.clone()));
                WorkItemSpec {
                    fn_ref: item.fn_ref.clone(),
                    function_name,
                    source_file,
                    kind: WorkItemKind::Concrete,
                    direct_args: item.args.clone(),
                    direct_kwargs: item.kwargs.clone(),
                    dag_inputs: HashMap::new(),
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                }
            })
            .collect();

        // Expand the plan (this suspends the parent internally)
        let (group, _child_ids) = self.plan.submit_parallel(parent_key, specs);

        // Move worker from active to suspended
        let worker = self.active.remove(&parent_key).unwrap();
        self.suspended.insert(
            parent_key,
            SuspendedWorker {
                child: worker.child,
                stream: worker.stream,
                items: worker.items,
                socket_path: worker.socket_path,
                group,
            },
        );

        // Initialize parallel results tracking
        let group_data = self.plan.group(group).unwrap();
        self.parallel_results
            .insert(group, vec![None; group_data.items.len()]);
    }

    fn resume_parent(&mut self, parent: WorkItemId, group: ParallelGroupId) {
        // group_item_done already called mark_resumed on the plan

        if let Some(suspended) = self.suspended.remove(&parent) {
            // Build the response
            let results: Vec<ParallelResult> = self
                .parallel_results
                .remove(&group)
                .unwrap_or_default()
                .into_iter()
                .map(|r| match r {
                    Some(val) => ParallelResult::Ok { result: val },
                    None => ParallelResult::Error {
                        error: "no result".to_string(),
                    },
                })
                .collect();

            let response = CoordinatorMessage::ParallelResponse { results };
            protocol::write_message(&mut &suspended.stream, &response).ok();

            // Move back to active
            self.active.insert(
                parent,
                ActiveWorker {
                    child: suspended.child,
                    stream: suspended.stream,
                    items: suspended.items,
                    socket_path: suspended.socket_path,
                    started_at: Instant::now(),
                },
            );
        }
    }

    fn handle_worker_disconnect(&mut self, key: WorkItemId) {
        if let Some(mut worker) = self.active.remove(&key) {
            // Mark all incomplete items as failed
            for &id in &worker.items {
                if self.plan.status(id) == WorkItemStatus::Running {
                    self.plan.mark_failed(id);
                }
            }
            // Wait for process to exit (don't leave zombies)
            worker.child.wait().ok();
            // Cleanup socket file
            std::fs::remove_file(&worker.socket_path).ok();
        }
    }

    fn find_item_by_fn_ref(&self, fn_ref: &str) -> Option<WorkItemId> {
        // Search active worker items for matching fn_ref
        for worker in self.active.values() {
            for &id in &worker.items {
                if self.plan.item(id).fn_ref == fn_ref {
                    return Some(id);
                }
            }
        }
        // Also search suspended workers (for parallel child completion)
        for worker in self.suspended.values() {
            for &id in &worker.items {
                if self.plan.item(id).fn_ref == fn_ref {
                    return Some(id);
                }
            }
        }
        None
    }
}

/// Check if an IO error is a would-block or timeout condition.
fn is_would_block_or_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}
