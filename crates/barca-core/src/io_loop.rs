//! Async I/O layer connecting the pure [`Coordinator`] to real OS processes via
//! tokio Unix sockets.
//!
//! This module's ONLY job: translate between Coordinator [`Action`]s and real I/O.
//! No logic, no decisions — all decision-making lives in the Coordinator.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::coordinator::{Action, Coordinator, ItemSpec};
use crate::protocol::{CoordinatorMessage, ParallelResult, WorkerMessage};

// ─── Configuration ───────────────────────────────────────────────────────────

/// Configuration for the I/O loop.
pub struct IoConfig {
    pub python: PathBuf,
    pub pool_size: usize,
    pub run_id: String,
}

// ─── Worker handle ───────────────────────────────────────────────────────────

/// A live worker process and its socket connection.
struct Worker {
    child: Child,
    stream: UnixStream,
    /// Whether this worker is currently executing (vs idle/waiting for item).
    busy: bool,
}

// ─── Events ──────────────────────────────────────────────────────────────────

/// An event received from the worker pool.
enum IoEvent {
    Message {
        worker_id: usize,
        msg: WorkerMessage,
    },
    Disconnected {
        worker_id: usize,
    },
}

// ─── Top-level entry point ───────────────────────────────────────────────────

/// Run the coordinator to completion using real processes.
/// This is the top-level entry point that replaces the old dispatch system.
pub async fn run(coord: &mut Coordinator, config: &IoConfig) -> Result<(), String> {
    // Create one shared listener for all workers
    let socket_path = crate::protocol::socket_path(&config.run_id, "main");
    std::fs::remove_file(&socket_path).ok();
    let listener = UnixListener::bind(&socket_path).map_err(|e| format!("socket bind: {e}"))?;

    // Spawn initial pool of worker processes
    let mut workers: HashMap<usize, Worker> = HashMap::new();
    for worker_id in 0..config.pool_size {
        match spawn_worker(&config.python, &socket_path, worker_id, &listener).await {
            Ok((child, stream)) => {
                workers.insert(
                    worker_id,
                    Worker {
                        child,
                        stream,
                        busy: false,
                    },
                );
            }
            Err(e) => {
                eprintln!("[barca] failed to spawn worker {worker_id}: {e}");
            }
        }
    }

    // Process initial ExecuteItem actions (workers need their first items)
    let initial_actions = kick_start(coord);
    execute_actions(
        &mut workers,
        coord,
        &initial_actions,
        &config.python,
        &socket_path,
        &listener,
    )
    .await;

    // Main event loop: wait for any worker to send a message, feed to coordinator
    loop {
        if coord.is_finished() {
            break;
        }

        // Check for deadlock
        if let Some(action) = coord.check_deadlock() {
            execute_actions(
                &mut workers,
                coord,
                &[action],
                &config.python,
                &socket_path,
                &listener,
            )
            .await;
        }

        // Wait for a message from ANY worker (tokio::select! across all streams)
        let event = wait_for_event(&mut workers).await;

        // Feed event to coordinator (pure logic)
        let actions = match event {
            IoEvent::Message { worker_id, msg } => handle_message(coord, worker_id, msg),
            IoEvent::Disconnected { worker_id } => coord.on_worker_crashed(worker_id),
        };

        // Execute resulting actions
        execute_actions(
            &mut workers,
            coord,
            &actions,
            &config.python,
            &socket_path,
            &listener,
        )
        .await;
    }

    // Cleanup
    for (_, mut worker) in workers {
        let _ = worker.child.wait();
    }
    std::fs::remove_file(&socket_path).ok();

    Ok(())
}

// ─── Kick-start ──────────────────────────────────────────────────────────────

/// For each worker that has items in its queue, pop the first and generate ExecuteItem.
fn kick_start(coord: &mut Coordinator) -> Vec<Action> {
    let mut actions = Vec::new();
    for worker_id in 0..coord.pool_size() {
        if coord.queue_len(worker_id) > 0 && !coord.is_executing(worker_id) {
            if let Some(item_id) = coord.pop_next(worker_id) {
                actions.push(Action::ExecuteItem { worker_id, item_id });
            }
        }
    }
    actions
}

// ─── Event waiting ───────────────────────────────────────────────────────────

/// Wait for a message from any connected worker.
///
/// Uses `futures::future::select_all` to multiplex reads across all busy workers.
async fn wait_for_event(workers: &mut HashMap<usize, Worker>) -> IoEvent {
    // Collect (worker_id, stream) pairs for busy workers.
    // We temporarily take the streams out to satisfy the borrow checker.
    let busy_ids: Vec<usize> = workers
        .iter()
        .filter(|(_, w)| w.busy)
        .map(|(&id, _)| id)
        .collect();

    if busy_ids.is_empty() {
        // No busy workers — should not happen if the coordinator is correct.
        // Sleep briefly and return; the outer loop will check is_finished.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // If we're here it's likely a transient state; return a disconnect for
        // worker 0 to force the coordinator to react.
        return IoEvent::Disconnected { worker_id: 0 };
    }

    // Since we can't easily extract streams from the HashMap without making them
    // Option<UnixStream>, we use a poll-with-timeout approach: try reading from
    // each busy worker with a short timeout, round-robin style. This is less
    // efficient than a true select_all but compiles cleanly and is correct.
    //
    // TODO: Refactor Worker.stream to Option<UnixStream> for true select_all.
    loop {
        for &id in &busy_ids {
            if let Some(worker) = workers.get_mut(&id) {
                // Try to read with a very short timeout
                let result = tokio::time::timeout(
                    Duration::from_millis(1),
                    read_one_message(&mut worker.stream),
                )
                .await;

                match result {
                    Ok(Ok(msg)) => return IoEvent::Message { worker_id: id, msg },
                    Ok(Err(_)) => return IoEvent::Disconnected { worker_id: id },
                    Err(_) => {
                        // Timeout — try next worker
                        continue;
                    }
                }
            }
        }
        // Yield to avoid busy-spinning
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

// ─── Message I/O ─────────────────────────────────────────────────────────────

/// Read one length-prefixed message from a worker stream.
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

/// Write a length-prefixed message to a worker stream.
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

// ─── Message handling ────────────────────────────────────────────────────────

/// Handle a WorkerMessage, translating to coordinator events.
fn handle_message(coord: &mut Coordinator, worker_id: usize, msg: WorkerMessage) -> Vec<Action> {
    match msg {
        WorkerMessage::StepCompleted {
            node_id: _,
            artifact: _,
        } => {
            let item_id = coord
                .executing_item(worker_id)
                .expect("completed message but worker not executing");
            coord.on_item_completed(worker_id, item_id)
        }
        WorkerMessage::StepError {
            node_id: _,
            message,
            ..
        } => {
            let item_id = coord
                .executing_item(worker_id)
                .expect("error message but worker not executing");
            coord.on_item_failed(worker_id, item_id, message)
        }
        WorkerMessage::Blocked { node_id: _, reason } => {
            let item_id = coord
                .executing_item(worker_id)
                .expect("blocked message but worker not executing");
            coord.on_item_failed(worker_id, item_id, format!("blocked: {reason}"))
        }
        WorkerMessage::Submit { items } => {
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
                        is_dynamic: false,
                    }
                })
                .collect();
            coord.on_parallel_requested(worker_id, specs)
        }
        WorkerMessage::Heartbeat => Vec::new(),
    }
}

// ─── Action execution ────────────────────────────────────────────────────────

/// Execute a list of actions by doing real I/O.
async fn execute_actions(
    workers: &mut HashMap<usize, Worker>,
    coord: &mut Coordinator,
    actions: &[Action],
    python: &Path,
    socket_path: &Path,
    listener: &UnixListener,
) {
    for action in actions {
        match action {
            Action::ExecuteItem { worker_id, item_id } => {
                if let Some(worker) = workers.get_mut(worker_id) {
                    let item = coord.item(*item_id);
                    let step = build_step_json(item);
                    let msg = serde_json::json!({"type": "execute", "step": step});
                    let _ = write_message(&mut worker.stream, &msg).await;
                    worker.busy = true;
                }
            }
            Action::SuspendWorker { worker_id } => {
                if let Some(worker) = workers.get_mut(worker_id) {
                    worker.busy = false;
                }
            }
            Action::ResumeWorker {
                worker_id,
                group_id,
            } => {
                if let Some(worker) = workers.get_mut(worker_id) {
                    let group = coord.group(*group_id);
                    let results: Vec<ParallelResult> = group
                        .items
                        .iter()
                        .map(|&iid| {
                            if coord.is_done(iid) {
                                ParallelResult::Ok {
                                    result: serde_json::Value::Null,
                                }
                            } else {
                                ParallelResult::Error {
                                    error: "failed".to_string(),
                                }
                            }
                        })
                        .collect();
                    let response = CoordinatorMessage::ParallelResponse { results };
                    let _ = write_message(&mut worker.stream, &response).await;
                    worker.busy = true;
                }
            }
            Action::WakeWorker { worker_id } => {
                if let Some(worker) = workers.get_mut(worker_id) {
                    if !worker.busy {
                        if let Some(item_id) = coord.pop_next(*worker_id) {
                            let item = coord.item(item_id);
                            let step = build_step_json(item);
                            let msg = serde_json::json!({"type": "execute", "step": step});
                            let _ = write_message(&mut worker.stream, &msg).await;
                            worker.busy = true;
                        }
                    }
                }
            }
            Action::RespawnWorker { worker_id } => {
                if let Some(mut old) = workers.remove(worker_id) {
                    let _ = old.child.kill();
                    let _ = old.child.wait();
                }
                match spawn_worker(python, socket_path, *worker_id, listener).await {
                    Ok((child, stream)) => {
                        workers.insert(
                            *worker_id,
                            Worker {
                                child,
                                stream,
                                busy: false,
                            },
                        );
                    }
                    Err(e) => {
                        eprintln!("[barca] respawn failed for worker {worker_id}: {e}");
                    }
                }
            }
            Action::SpawnTempWorker { items: _ } => {
                let temp_id = workers.keys().max().copied().unwrap_or(0) + 1;
                match spawn_worker(python, socket_path, temp_id, listener).await {
                    Ok((child, stream)) => {
                        workers.insert(
                            temp_id,
                            Worker {
                                child,
                                stream,
                                busy: false,
                            },
                        );
                    }
                    Err(e) => {
                        eprintln!("[barca] temp worker spawn failed: {e}");
                    }
                }
            }
        }
    }
}

// ─── Worker spawning ─────────────────────────────────────────────────────────

/// Spawn a worker process and accept its socket connection.
///
/// The worker connects to the shared Unix socket listener. We accept the
/// connection with a timeout and return the (child, stream) pair.
async fn spawn_worker(
    python: &Path,
    socket_path: &Path,
    worker_id: usize,
    listener: &UnixListener,
) -> Result<(Child, UnixStream), String> {
    let socket_str = socket_path.to_str().unwrap_or("");

    let child = Command::new(python)
        .args(["-m", "barca._worker", "--daemon"])
        .env("BARCA_SOCKET", socket_str)
        .env("BARCA_WORKER", "1")
        .env("BARCA_WORKER_ID", worker_id.to_string())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;

    // Accept the worker's connection with a timeout.
    // NOTE: In a full implementation the worker sends a hello message with its ID
    // so we can match connections to workers. For now, accept in spawn order.
    let stream = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .map_err(|_| format!("timeout waiting for worker {worker_id} to connect"))?
        .map_err(|e| format!("accept: {e}"))?
        .0;

    Ok((child, stream))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build the step JSON to send to a worker for execution.
fn build_step_json(item: &crate::coordinator::Item) -> serde_json::Value {
    serde_json::json!({
        "node_id": format!("{}[_branch={}]", item.spec.fn_ref, item.id.0),
        "function_name": item.spec.function_name,
        "source_file": item.spec.source_file,
        "kind": "task",
        "inputs": item.spec.dag_inputs,
        "timeout_seconds": item.spec.timeout_seconds,
        "direct_args": item.spec.direct_args,
        "direct_kwargs": item.spec.direct_kwargs,
        "serializer": "json",
    })
}
