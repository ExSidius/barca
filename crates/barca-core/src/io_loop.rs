//! Async I/O layer — stateless workers pulling from a global ready queue.
//!
//! Workers are ephemeral executors. After each task, they report back and Rust
//! assigns the next ready item. On parallel(), the requesting worker is SIGSTOP'd,
//! a temp replacement is spawned, and children enter the ready queue. When all
//! children complete, the temp is killed and the original is SIGCONT'd.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::coordinator::{Coordinator, GroupId, ItemId, ItemSpec};
use crate::protocol::{CoordinatorMessage, ParallelResult, WorkerMessage};

// ─── Configuration ───────────────────────────────────────────────────────────

pub struct IoConfig {
    pub python: PathBuf,
    pub pool_size: usize,
    pub run_id: String,
}

/// Callback invoked on each step completion with (node_id, artifact_json).
pub type StepCallback<'a> = Box<dyn FnMut(&str, &serde_json::Value) + 'a>;

// ─── Worker handle ───────────────────────────────────────────────────────────

struct WorkerHandle {
    child: Child,
    cmd_tx: mpsc::Sender<serde_json::Value>,
    _task: JoinHandle<()>,
    /// The item this worker is currently executing.
    executing: Option<ItemId>,
}

// ─── Frozen worker (SIGSTOP'd, waiting for parallel group) ──────────────────

struct FrozenWorker {
    child: Child,
    cmd_tx: mpsc::Sender<serde_json::Value>,
    _task: JoinHandle<()>,
    parent_item: ItemId,
    group_id: GroupId,
    /// The worker_id used when this worker was originally spawned (matches
    /// the worker_io_task's worker_id, so events arrive with this key).
    original_worker_id: usize,
    /// The active worker ID that replaced this frozen one.
    replacement_id: usize,
}

// ─── Events ──────────────────────────────────────────────────────────────────

enum IoEvent {
    Message {
        worker_id: usize,
        msg: WorkerMessage,
    },
    Disconnected {
        worker_id: usize,
    },
}

// ─── Worker I/O task ─────────────────────────────────────────────────────────

async fn worker_io_task(
    worker_id: usize,
    mut stream: UnixStream,
    mut cmd_rx: mpsc::Receiver<serde_json::Value>,
    event_tx: mpsc::Sender<IoEvent>,
) {
    loop {
        tokio::select! {
            result = read_one_message(&mut stream) => {
                match result {
                    Ok(msg) => {
                        if event_tx.send(IoEvent::Message { worker_id, msg }).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = event_tx.send(IoEvent::Disconnected { worker_id }).await;
                        break;
                    }
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(msg) => {
                        if write_message(&mut stream, &msg).await.is_err() {
                            let _ = event_tx.send(IoEvent::Disconnected { worker_id }).await;
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

// ─── Top-level entry point ───────────────────────────────────────────────────

pub async fn run(
    coord: &mut Coordinator,
    config: &IoConfig,
    mut on_step: Option<StepCallback<'_>>,
) -> Result<(), String> {
    let socket_path = crate::protocol::socket_path(&config.run_id, "main");
    std::fs::remove_file(&socket_path).ok();
    let listener = UnixListener::bind(&socket_path).map_err(|e| format!("socket bind: {e}"))?;

    let (event_tx, mut event_rx) = mpsc::channel::<IoEvent>(config.pool_size * 8);

    // Spawn initial worker pool (only as many as there are ready items, up to pool_size)
    let initial_count = coord.ready_count().min(config.pool_size).max(1);
    let mut workers: HashMap<usize, WorkerHandle> = HashMap::new();
    let mut next_worker_id: usize = 0;

    for _ in 0..initial_count {
        let wid = next_worker_id;
        next_worker_id += 1;
        match spawn_worker(&config.python, &socket_path, wid, &listener, &event_tx).await {
            Ok(handle) => {
                workers.insert(wid, handle);
            }
            Err(e) => eprintln!("[barca] failed to spawn worker {wid}: {e}"),
        }
    }

    // Frozen workers (SIGSTOP'd, waiting for parallel groups)
    let mut frozen: Vec<FrozenWorker> = Vec::new();

    // Assign initial ready items to idle workers
    assign_ready(
        &mut workers,
        coord,
        &config.python,
        &socket_path,
        &listener,
        &event_tx,
        &mut next_worker_id,
    )
    .await;

    // Main event loop
    loop {
        if coord.is_finished() {
            break;
        }

        let event = match event_rx.recv().await {
            Some(e) => e,
            None => break,
        };

        match event {
            IoEvent::Message { worker_id, msg } => {
                match msg {
                    WorkerMessage::StepCompleted {
                        ref node_id,
                        ref artifact,
                    } => {
                        let item_id = workers
                            .get(&worker_id)
                            .and_then(|w| w.executing)
                            .expect("StepCompleted but worker has no executing item");

                        // Record output and fire progress callback
                        let artifact_val = serde_json::to_value(artifact).unwrap_or_default();
                        coord.record_output(item_id, artifact_val.clone());
                        if let Some(ref mut cb) = on_step {
                            cb(node_id, &artifact_val);
                        }
                        coord.on_item_completed(item_id);

                        // Mark worker idle
                        if let Some(w) = workers.get_mut(&worker_id) {
                            w.executing = None;
                        }

                        // Check if any frozen worker's group is now complete
                        resume_frozen(&mut frozen, &mut workers, coord).await;

                        // Assign next ready items
                        assign_ready(
                            &mut workers,
                            coord,
                            &config.python,
                            &socket_path,
                            &listener,
                            &event_tx,
                            &mut next_worker_id,
                        )
                        .await;
                    }
                    WorkerMessage::StepError { message, .. } => {
                        let item_id = workers
                            .get(&worker_id)
                            .and_then(|w| w.executing)
                            .expect("StepError but worker has no executing item");

                        coord.on_item_failed(item_id, message);

                        if let Some(w) = workers.get_mut(&worker_id) {
                            w.executing = None;
                        }

                        resume_frozen(&mut frozen, &mut workers, coord).await;
                        assign_ready(
                            &mut workers,
                            coord,
                            &config.python,
                            &socket_path,
                            &listener,
                            &event_tx,
                            &mut next_worker_id,
                        )
                        .await;
                    }
                    WorkerMessage::Blocked { reason, .. } => {
                        let item_id = workers
                            .get(&worker_id)
                            .and_then(|w| w.executing)
                            .expect("Blocked but worker has no executing item");

                        coord.on_item_failed(item_id, format!("blocked: {reason}"));

                        if let Some(w) = workers.get_mut(&worker_id) {
                            w.executing = None;
                        }

                        resume_frozen(&mut frozen, &mut workers, coord).await;
                        assign_ready(
                            &mut workers,
                            coord,
                            &config.python,
                            &socket_path,
                            &listener,
                            &event_tx,
                            &mut next_worker_id,
                        )
                        .await;
                    }
                    WorkerMessage::Submit { items } => {
                        let item_id = workers
                            .get(&worker_id)
                            .and_then(|w| w.executing)
                            .expect("Submit but worker has no executing item");

                        let specs: Vec<ItemSpec> = items
                            .into_iter()
                            .map(|si| {
                                let (source_file, function_name) = si
                                    .fn_ref
                                    .rsplit_once(':')
                                    .map(|(f, n)| (f.to_string(), n.to_string()))
                                    .unwrap_or_else(|| (String::new(), si.fn_ref.clone()));
                                ItemSpec {
                                    fn_ref: si.fn_ref,
                                    function_name,
                                    source_file,
                                    direct_args: si.args,
                                    direct_kwargs: si.kwargs,
                                    dag_inputs: HashMap::new(),
                                    timeout_seconds: 300,
                                    retries: 1,
                                    retry_backoff_seconds: 0.0,
                                    serializer: None,
                                    upstream_inputs: HashMap::new(),
                                    is_dynamic: false,
                                }
                            })
                            .collect();

                        let (group_id, _child_ids) = coord.on_parallel_requested(item_id, specs);

                        // SIGSTOP the requesting worker, move it to frozen list
                        let Some(handle) = workers.remove(&worker_id) else {
                            eprintln!("[barca] Submit from unknown worker {worker_id}, ignoring");
                            continue;
                        };
                        #[cfg(unix)]
                        unsafe {
                            libc::kill(handle.child.id() as i32, libc::SIGSTOP);
                        }

                        // Spawn a replacement worker in the same slot
                        let replacement_id = next_worker_id;
                        next_worker_id += 1;
                        match spawn_worker(
                            &config.python,
                            &socket_path,
                            replacement_id,
                            &listener,
                            &event_tx,
                        )
                        .await
                        {
                            Ok(replacement) => {
                                workers.insert(replacement_id, replacement);
                            }
                            Err(e) => eprintln!("[barca] failed to spawn replacement worker: {e}"),
                        }

                        frozen.push(FrozenWorker {
                            child: handle.child,
                            cmd_tx: handle.cmd_tx,
                            _task: handle._task,
                            parent_item: item_id,
                            group_id,
                            original_worker_id: worker_id,
                            replacement_id,
                        });

                        // Assign ready items (children are now in the ready queue)
                        assign_ready(
                            &mut workers,
                            coord,
                            &config.python,
                            &socket_path,
                            &listener,
                            &event_tx,
                            &mut next_worker_id,
                        )
                        .await;
                    }
                    WorkerMessage::Heartbeat => {}
                }
            }
            IoEvent::Disconnected { worker_id } => {
                // Worker crashed — fail its executing item
                if let Some(handle) = workers.get(&worker_id) {
                    if let Some(item_id) = handle.executing {
                        coord.on_item_failed(item_id, "worker disconnected".to_string());
                    }
                }
                workers.remove(&worker_id);
                // Spawn replacement
                let wid = next_worker_id;
                next_worker_id += 1;
                if let Ok(h) =
                    spawn_worker(&config.python, &socket_path, wid, &listener, &event_tx).await
                {
                    workers.insert(wid, h);
                }
                resume_frozen(&mut frozen, &mut workers, coord).await;
                assign_ready(
                    &mut workers,
                    coord,
                    &config.python,
                    &socket_path,
                    &listener,
                    &event_tx,
                    &mut next_worker_id,
                )
                .await;
            }
        }
    }

    // Cleanup: kill all active and frozen workers
    for (_, mut w) in workers {
        let _ = w.child.kill();
        let _ = w.child.wait();
    }
    for mut fw in frozen {
        #[cfg(unix)]
        unsafe {
            libc::kill(fw.child.id() as i32, libc::SIGCONT);
        }
        let _ = fw.child.kill();
        let _ = fw.child.wait();
    }
    std::fs::remove_file(&socket_path).ok();

    Ok(())
}

// ─── Assign ready items to idle workers ──────────────────────────────────────

async fn assign_ready(
    workers: &mut HashMap<usize, WorkerHandle>,
    coord: &mut Coordinator,
    python: &Path,
    socket_path: &Path,
    listener: &UnixListener,
    event_tx: &mpsc::Sender<IoEvent>,
    next_worker_id: &mut usize,
) {
    // Collect idle worker IDs
    let idle: Vec<usize> = workers
        .iter()
        .filter(|(_, w)| w.executing.is_none())
        .map(|(&id, _)| id)
        .collect();

    for wid in idle {
        if let Some(item_id) = coord.next_ready() {
            let item = coord.item(item_id);
            let step = build_step_json(item, coord);
            let msg = serde_json::json!({"type": "execute", "step": step});
            if let Some(w) = workers.get_mut(&wid) {
                let _ = w.cmd_tx.send(msg).await;
                w.executing = Some(item_id);
            }
        }
    }

    // If there are more ready items than idle workers, spawn additional workers
    // (up to pool_size total active workers — not counting frozen)
    while coord.ready_count() > 0 {
        // Don't exceed reasonable pool limits
        if workers.len() >= 64 {
            break;
        }
        let wid = *next_worker_id;
        *next_worker_id += 1;
        match spawn_worker(python, socket_path, wid, listener, event_tx).await {
            Ok(mut handle) => {
                if let Some(item_id) = coord.next_ready() {
                    let item = coord.item(item_id);
                    let step = build_step_json(item, coord);
                    let msg = serde_json::json!({"type": "execute", "step": step});
                    let _ = handle.cmd_tx.send(msg).await;
                    handle.executing = Some(item_id);
                }
                workers.insert(wid, handle);
            }
            Err(e) => {
                eprintln!("[barca] failed to spawn worker: {e}");
                break;
            }
        }
    }
}

// ─── Resume frozen workers whose groups completed ────────────────────────────

async fn resume_frozen(
    frozen: &mut Vec<FrozenWorker>,
    workers: &mut HashMap<usize, WorkerHandle>,
    coord: &mut Coordinator,
) {
    // Partition: completed groups get drained out
    let mut i = 0;
    while i < frozen.len() {
        if coord.is_group_complete(frozen[i].group_id) {
            let fw = frozen.swap_remove(i);

            // Kill the replacement worker
            if let Some(mut replacement) = workers.remove(&fw.replacement_id) {
                let _ = replacement.child.kill();
                let _ = replacement.child.wait();
            }

            // SIGCONT the original worker
            #[cfg(unix)]
            unsafe {
                libc::kill(fw.child.id() as i32, libc::SIGCONT);
            }

            // Build ParallelResponse with results for each child.
            // Read actual JSON values from the artifact files so the parent
            // task receives the real return values (not Null).
            let group = coord.group(fw.group_id);
            let results: Vec<ParallelResult> = group
                .items
                .iter()
                .map(|&iid| {
                    if coord.is_done(iid) {
                        let val = coord
                            .outputs()
                            .get(&iid)
                            .and_then(|artifact| {
                                let fmt = artifact
                                    .get("format")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let path =
                                    artifact.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                if fmt == "json" && !path.is_empty() {
                                    std::fs::read_to_string(path)
                                        .ok()
                                        .and_then(|s| serde_json::from_str(&s).ok())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(serde_json::Value::Null);
                        ParallelResult::Ok { result: val }
                    } else {
                        ParallelResult::Error {
                            error: "failed".to_string(),
                        }
                    }
                })
                .collect();
            let response = CoordinatorMessage::ParallelResponse { results };
            let msg = serde_json::to_value(&response).unwrap_or_default();
            let _ = fw.cmd_tx.send(msg).await;

            // Re-insert using the original worker_id — the worker_io_task
            // was spawned with this ID, so StepCompleted events arrive keyed
            // by it.
            workers.insert(
                fw.original_worker_id,
                WorkerHandle {
                    child: fw.child,
                    cmd_tx: fw.cmd_tx,
                    _task: fw._task,
                    executing: Some(fw.parent_item), // still executing the parent task
                },
            );
            // Don't increment i — swap_remove moved the last element here
        } else {
            i += 1;
        }
    }
}

// ─── Message I/O ─────────────────────────────────────────────────────────────

async fn read_one_message(stream: &mut UnixStream) -> Result<WorkerMessage, std::io::Error> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 256 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

async fn write_message(
    stream: &mut UnixStream,
    msg: &impl serde::Serialize,
) -> Result<(), std::io::Error> {
    let payload = serde_json::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let header = (payload.len() as u32).to_be_bytes();
    stream.write_all(&header).await?;
    stream.write_all(&payload).await?;
    stream.flush().await
}

// ─── Worker spawning ─────────────────────────────────────────────────────────

async fn spawn_worker(
    python: &Path,
    socket_path: &Path,
    worker_id: usize,
    listener: &UnixListener,
    event_tx: &mpsc::Sender<IoEvent>,
) -> Result<WorkerHandle, String> {
    let child = Command::new(python)
        .args(["-m", "barca._worker", "--daemon"])
        .env("BARCA_SOCKET", socket_path.to_str().unwrap_or(""))
        .env("BARCA_WORKER", "1")
        .env("BARCA_WORKER_ID", worker_id.to_string())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;

    let stream = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .map_err(|_| format!("timeout waiting for worker {worker_id} to connect"))?
        .map_err(|e| format!("accept: {e}"))?
        .0;

    let (cmd_tx, cmd_rx) = mpsc::channel::<serde_json::Value>(16);
    let etx = event_tx.clone();
    let task = tokio::spawn(worker_io_task(worker_id, stream, cmd_rx, etx));

    Ok(WorkerHandle {
        child,
        cmd_tx,
        _task: task,
        executing: None,
    })
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_step_json(item: &crate::coordinator::Item, coord: &Coordinator) -> serde_json::Value {
    // Start with dag_inputs from spec (cross-phase provided inputs).
    let mut inputs = item.spec.dag_inputs.clone();

    // Fill in-phase upstream artifacts: for params that don't have a path yet,
    // look up the coordinator's outputs using the upstream_inputs mapping.
    for (param, upstream_node_id) in &item.spec.upstream_inputs {
        if inputs.contains_key(param) && !inputs[param].is_empty() {
            continue;
        }
        for (&uid, artifact) in coord.outputs() {
            let upstream_item = coord.item(uid);
            if upstream_item.step_id.display() == *upstream_node_id {
                let path = artifact.get("path").and_then(|v| v.as_str()).unwrap_or("");
                if !path.is_empty() {
                    inputs.insert(param.clone(), path.to_string());
                }
                break;
            }
        }
    }

    serde_json::json!({
        "node_id": item.step_id.display(),
        "function_name": item.spec.function_name,
        "source_file": item.spec.source_file,
        "kind": "task",
        "inputs": inputs,
        "timeout_seconds": item.spec.timeout_seconds,
        "direct_args": item.spec.direct_args,
        "direct_kwargs": item.spec.direct_kwargs,
        "serializer": item.spec.serializer.as_deref().unwrap_or("json"),
    })
}
