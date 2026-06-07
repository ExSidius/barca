//! Phase execution engine — worker spawning, protocol parsing, partition expansion.

use crate::planner::{Phase, StreamStep, WorkerStream, expand_partition_combos};
use crate::scheduler::Unit;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Reference to a materialized artifact on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputRef {
    pub path: String,
    pub format: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_seconds: Option<f64>,
}

/// A structured failure reported by a worker for a single step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepError {
    pub error_type: String,
    pub message: String,
    /// User-relevant traceback (barca-internal frames filtered out).
    pub traceback: String,
    /// Number of attempts made before this failure became permanent.
    pub attempts: u32,
}

/// A work item from a `parallel()` request — one branch to dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelWorkItem {
    /// Function reference: "source_file.py:function_name"
    pub fn_ref: String,
    /// Positional args (JSON values).
    pub args: Vec<serde_json::Value>,
    /// Keyword args (JSON values).
    pub kwargs: HashMap<String, serde_json::Value>,
}

/// A permanently-failed step (retry budget exhausted), for the metadata DB.
#[derive(Debug, Clone, PartialEq)]
pub struct StepFailure {
    pub node_id: String,
    pub error: StepError,
}

/// Expand steps with pending_partitions using materialized source outputs.
/// Returns None if no expansion needed, or a new Phase with expanded steps.
pub fn expand_pending_partitions(
    phase: &Phase,
    all_outputs: &HashMap<String, OutputRef>,
    pool_size: usize,
) -> Option<Phase> {
    let has_pending = phase
        .streams
        .iter()
        .any(|s| s.steps.iter().any(|st| !st.pending_partitions.is_empty()));

    if !has_pending {
        return None;
    }

    let mut all_expanded_steps: Vec<StreamStep> = Vec::new();
    let mut passthrough_steps: Vec<StreamStep> = Vec::new();

    for stream in &phase.streams {
        for step in &stream.steps {
            if step.pending_partitions.is_empty() {
                passthrough_steps.push(step.clone());
                continue;
            }

            let mut dim_values: HashMap<String, Vec<String>> = HashMap::new();
            for (dim, source_name) in &step.pending_partitions {
                let source_ref = all_outputs
                    .iter()
                    .find(|(k, _)| {
                        k.ends_with(&format!(":{source_name}"))
                            || k.as_str() == source_name.as_str()
                    })
                    .map(|(_, v)| v);

                if let Some(oref) = source_ref {
                    if oref.format != "json" {
                        eprintln!(
                            "[barca] Error: partition source '{}' must be JSON format, got '{}'",
                            source_name, oref.format
                        );
                        continue;
                    }
                    // Read the JSON artifact file from disk.
                    let json_str = match std::fs::read_to_string(&oref.path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!(
                                "[barca] Error: failed to read partition artifact '{}': {e}",
                                oref.path
                            );
                            continue;
                        }
                    };
                    let parsed: serde_json::Value =
                        serde_json::from_str(&json_str).unwrap_or_default();
                    let values: Vec<String> = match parsed {
                        serde_json::Value::Array(arr) => arr
                            .iter()
                            .filter_map(|v| match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Number(n) => Some(n.to_string()),
                                _ => None,
                            })
                            .collect(),
                        _ => {
                            eprintln!(
                                "[barca] Warning: partition source '{}' did not return an array",
                                source_name
                            );
                            continue;
                        }
                    };
                    dim_values.insert(dim.clone(), values);
                } else {
                    eprintln!(
                        "[barca] Warning: partition source '{}' not found in outputs",
                        source_name
                    );
                }
            }

            if dim_values.is_empty() {
                passthrough_steps.push(step.clone());
                continue;
            }

            let combos = expand_partition_combos(&dim_values);
            let pks: Vec<crate::PartitionKey> =
                combos.into_iter().map(crate::PartitionKey::from).collect();
            all_expanded_steps.push(StreamStep {
                step_id: crate::StepId::unpartitioned(step.step_id.base.clone()),
                kind: step.kind,
                function_name: step.function_name.clone(),
                source_file: step.source_file.clone(),
                inputs: step.inputs.clone(),
                pending_partitions: HashMap::new(),
                serializer: step.serializer.clone(),
                timeout_seconds: step.timeout_seconds,
                retries: step.retries,
                retry_backoff_seconds: step.retry_backoff_seconds,
                partition_keys: pks,
            });
        }
    }

    // Build work units: expanded steps (with partition_keys) + passthrough steps.
    let mut work_units: Vec<Vec<StreamStep>> = Vec::new();
    if !passthrough_steps.is_empty() {
        work_units.push(passthrough_steps);
    }
    // Split each expanded step's partition_keys across streams.
    for step in all_expanded_steps {
        let total_pks = step.partition_keys.len();
        if total_pks == 0 {
            work_units.push(vec![step]);
        } else {
            let chunk_size = total_pks.div_ceil(pool_size);
            for chunk in step.partition_keys.chunks(chunk_size) {
                work_units.push(vec![StreamStep {
                    step_id: step.step_id.clone(),
                    kind: step.kind,
                    function_name: step.function_name.clone(),
                    source_file: step.source_file.clone(),
                    inputs: step.inputs.clone(),
                    pending_partitions: step.pending_partitions.clone(),
                    serializer: step.serializer.clone(),
                    timeout_seconds: step.timeout_seconds,
                    retries: step.retries,
                    retry_backoff_seconds: step.retry_backoff_seconds,
                    partition_keys: chunk.to_vec(),
                }]);
            }
        }
    }

    // Distribute work units across streams via bin-packing.
    let num_streams = work_units.len().min(pool_size).max(1);
    let mut streams: Vec<Vec<StreamStep>> = vec![Vec::new(); num_streams];
    let mut sizes: Vec<usize> = vec![0; num_streams];

    for unit in work_units {
        let weight: usize = unit
            .iter()
            .map(|st| {
                if st.partition_keys.is_empty() {
                    1
                } else {
                    st.partition_keys.len()
                }
            })
            .sum();
        let target = sizes
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| *s)
            .map(|(i, _)| i)
            .unwrap_or(0);
        sizes[target] += weight;
        streams[target].extend(unit);
    }

    let worker_streams: Vec<WorkerStream> = streams
        .into_iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, steps)| WorkerStream {
            stream_id: format!("expanded-w{i}"),
            steps,
        })
        .collect();

    Some(Phase {
        reason: phase.reason.clone(),
        streams: worker_streams,
    })
}

/// A provided input — either a single artifact or a collected list of partition artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProvidedInput {
    Single(OutputRef),
    Collected(Vec<OutputRef>),
}

/// Determine what values need to be provided to workers in this phase.
///
/// Only data edges (`Direct`/`Collect`, surfaced via `step.inputs`) produce
/// provided inputs. `After` edges carry no data — they never appear in
/// `step.inputs`, so an after-ordered step receives no artifact from them.
pub fn build_provided_inputs(
    phase: &Phase,
    all_outputs: &HashMap<String, OutputRef>,
) -> HashMap<String, ProvidedInput> {
    let mut provided: HashMap<String, ProvidedInput> = HashMap::new();

    for stream in &phase.streams {
        for step in &stream.steps {
            let suffix = if step.step_id.partition.is_empty() {
                String::new()
            } else {
                format!("[{}]", step.step_id.partition.suffix())
            };

            for upstream_id in step.inputs.values() {
                // Direct base match.
                if let Some(output) = all_outputs.get(upstream_id) {
                    provided
                        .entry(upstream_id.clone())
                        .or_insert_with(|| ProvidedInput::Single(output.clone()));
                    continue;
                }
                // Partition-aligned match (old-style: step_id has partition suffix).
                if !suffix.is_empty() {
                    let aligned_id = format!("{upstream_id}{suffix}");
                    if let Some(output) = all_outputs.get(&aligned_id) {
                        provided
                            .entry(aligned_id)
                            .or_insert_with(|| ProvidedInput::Single(output.clone()));
                        continue;
                    }
                }
                // Late expansion: step has partition_keys — provide all aligned upstream
                // outputs so the worker can resolve per-partition inputs.
                if !step.partition_keys.is_empty() {
                    let mut found_any = false;
                    for pk in &step.partition_keys {
                        let pk_suffix = format!("[{}]", pk.suffix());
                        let aligned_id = format!("{upstream_id}{pk_suffix}");
                        if let Some(output) = all_outputs.get(&aligned_id) {
                            provided
                                .entry(aligned_id)
                                .or_insert_with(|| ProvidedInput::Single(output.clone()));
                            found_any = true;
                        }
                    }
                    if found_any {
                        continue;
                    }
                }
                // Fan-in collect(): upstream is partitioned but consumer is unpartitioned.
                // Gather all partition-suffixed outputs for this base id.
                let prefix = format!("{upstream_id}[");
                let mut collected: Vec<OutputRef> = all_outputs
                    .iter()
                    .filter(|(k, _)| k.starts_with(&prefix))
                    .map(|(_, v)| v.clone())
                    .collect();
                if !collected.is_empty() {
                    collected.sort_by(|a, b| a.path.cmp(&b.path));
                    provided
                        .entry(upstream_id.clone())
                        .or_insert(ProvidedInput::Collected(collected));
                }
            }
        }
    }

    provided
}

/// Result of executing a phase — may contain partial outputs if a worker failed.
pub struct PhaseResult {
    pub outputs: HashMap<String, OutputRef>,
    /// First permanent failure rendered as a string, for `BarcaError::WorkerFailed`
    /// back-compat. `None` if the phase fully succeeded.
    pub error: Option<String>,
    /// All permanently-failed steps (retry budget exhausted), for the metadata DB.
    pub failures: Vec<StepFailure>,
    /// Attempts dispatched per base node id — used to persist `attempts` for
    /// succeeded nodes (defaults to 1 when absent).
    pub attempts: HashMap<String, u32>,
}

/// Event from a worker thread to the coordinator. `StepCompleted` is streamed
/// live for progress; `UnitFinished` is fed to the pure scheduler core.
enum WorkerEvent {
    /// A step completed successfully (live progress).
    StepCompleted { node_id: String, output: OutputRef },
    /// A worker process for `unit` exited; carries the steps that failed/were blocked.
    UnitFinished {
        unit: Unit,
        failures: Vec<(String, StepError)>,
        blocked: Vec<String>,
    },
    /// A worker is requesting parallel dispatch of sub-tasks.
    /// The worker is suspended (blocking on stdin) until we write a response.
    #[allow(dead_code)]
    ParallelRequest {
        /// The worker's stdin pipe to write the response to when done.
        response_pipe: std::process::ChildStdin,
        /// The parent unit (kept alive while waiting).
        parent_unit: Unit,
        /// The work items to dispatch.
        items: Vec<ParallelWorkItem>,
    },
}

/// Execute a single phase. The pure `scheduler::Scheduler` owns all dispatch /
/// retry / backoff decisions; this function is the **imperative shell** that
/// interprets the scheduler's `Action`s (spawn a worker, arm a timer, record a
/// failure) and feeds `SchedEvent`s back in. Worker threads are pure producers
/// that stream `WorkerEvent`s over a single channel.
///
/// Callback type for per-step progress updates.
pub type StepCallback<'a> = &'a mut dyn FnMut(&str, &OutputRef);

/// `on_step` is called on the coordinator thread each time a step completes —
/// real-time progress even while multiple workers run in parallel.
pub fn execute_phase(
    phase: &Phase,
    provided_inputs: &HashMap<String, ProvidedInput>,
    python: &PathBuf,
    mut on_step: Option<StepCallback<'_>>,
) -> PhaseResult {
    use crate::scheduler::{Action, SchedEvent, Scheduler, Unit};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Instant;

    // Initial units = the phase's streams as-is (batched, no per-chain spawn).
    let initial_units: Vec<Unit> = phase
        .streams
        .iter()
        .map(|s| Unit {
            stream_id: s.stream_id.clone(),
            steps: s.steps.clone(),
            provided_ids: Vec::new(),
            retry_root: None,
        })
        .collect();
    // Capacity = number of streams (≈ pool_size); matches today's "one process
    // per stream concurrently", with freed slots reused for retries.
    let capacity = initial_units.len().max(1);

    let (tx, rx) = mpsc::channel::<WorkerEvent>();
    let mut sched = Scheduler::new(initial_units, capacity);

    let mut phase_outputs: HashMap<String, OutputRef> = HashMap::new();
    let mut failures: Vec<StepFailure> = Vec::new();
    // Actual executions per base node id (one per `result`/`error` event), for the
    // DB `attempts` column. Counts executions, not unit dispatches — a step blocked
    // while an upstream retries never executed, so it isn't counted.
    let mut exec_attempts: HashMap<String, u32> = HashMap::new();
    // Set by every `interpret!` (reset to None, then to the nearest backoff deadline).
    let mut next_timer: Option<Instant>;
    let mut spawn_error: Option<String> = None;
    let mut done = false;

    // Interpret a batch of scheduler actions. Returns true if `Finished` seen.
    macro_rules! interpret {
        ($actions:expr) => {{
            next_timer = None;
            for action in $actions {
                match action {
                    Action::Spawn(unit) => {
                        let spawn_result =
                            spawn_unit(&unit, provided_inputs, &phase_outputs, python, &tx);
                        if let Err(ref msg) = spawn_result {
                            spawn_error.get_or_insert(msg.clone());
                            // Synthesize a failure event so the scheduler decrements in_flight
                            let synth_failures: Vec<(String, StepError)> = unit
                                .steps
                                .iter()
                                .map(|s| {
                                    (
                                        s.step_id.display(),
                                        StepError {
                                            error_type: "SpawnError".to_string(),
                                            message: msg.clone(),
                                            traceback: String::new(),
                                            attempts: 0,
                                        },
                                    )
                                })
                                .collect();
                            let synth_event = SchedEvent::UnitFinished {
                                unit: unit.clone(),
                                failures: synth_failures,
                                blocked: vec![],
                            };
                            // Feed synthesized event directly into the scheduler
                            let synth_actions = sched.on_event(synth_event, Instant::now());
                            // Process any resulting actions (recursion-safe: spawn
                            // failures won't produce another Spawn for the same unit)
                            for sa in synth_actions {
                                match sa {
                                    Action::Spawn(u) => {
                                        if let Err(e2) = spawn_unit(
                                            &u,
                                            provided_inputs,
                                            &phase_outputs,
                                            python,
                                            &tx,
                                        ) {
                                            spawn_error.get_or_insert(e2);
                                        }
                                    }
                                    Action::SetTimer(at) => next_timer = Some(at),
                                    Action::EmitFailed(node_id, error) => {
                                        failures.push(StepFailure { node_id, error });
                                    }
                                    Action::Finished => done = true,
                                }
                            }
                        }
                    }
                    Action::SetTimer(at) => next_timer = Some(at),
                    Action::EmitFailed(node_id, error) => {
                        failures.push(StepFailure { node_id, error });
                    }
                    Action::Finished => done = true,
                }
            }
        }};
    }

    interpret!(sched.start(Instant::now()));

    while !done {
        let event: Option<SchedEvent> = match next_timer {
            Some(at) => {
                let now = Instant::now();
                if at <= now {
                    Some(SchedEvent::Tick)
                } else {
                    match rx.recv_timeout(at - now) {
                        Ok(we) => we.into_sched_event(
                            &mut on_step,
                            &mut phase_outputs,
                            &mut exec_attempts,
                        ),
                        Err(RecvTimeoutError::Timeout) => Some(SchedEvent::Tick),
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
            None => match rx.recv() {
                Ok(we) => we.into_sched_event(&mut on_step, &mut phase_outputs, &mut exec_attempts),
                Err(_) => break,
            },
        };

        let Some(event) = event else { continue };
        interpret!(sched.on_event(event, Instant::now()));
    }

    // Drop our sender; any still-running worker threads detach harmlessly.
    drop(tx);

    let error = spawn_error.or_else(|| {
        failures.first().map(|f| {
            if f.error.traceback.is_empty() {
                format!("{}: {}", f.error.error_type, f.error.message)
            } else {
                f.error.traceback.clone()
            }
        })
    });
    PhaseResult {
        outputs: phase_outputs,
        error,
        failures,
        attempts: exec_attempts,
    }
}

impl WorkerEvent {
    /// Fold a worker event into the coordinator: `StepCompleted` is handled
    /// inline (progress + record output) and yields `None`; `UnitFinished`
    /// becomes a `SchedEvent` for the pure core.
    fn into_sched_event(
        self,
        on_step: &mut Option<StepCallback<'_>>,
        phase_outputs: &mut HashMap<String, OutputRef>,
        exec_attempts: &mut HashMap<String, u32>,
    ) -> Option<crate::scheduler::SchedEvent> {
        match self {
            WorkerEvent::StepCompleted { node_id, output } => {
                // One actual execution of this node's function.
                let base = crate::StepId::parse(&node_id).base_id().to_string();
                *exec_attempts.entry(base).or_insert(0) += 1;
                if let Some(cb) = on_step {
                    cb(&node_id, &output);
                }
                phase_outputs.insert(node_id, output);
                None
            }
            WorkerEvent::UnitFinished {
                unit,
                failures,
                blocked,
            } => {
                // Each reported failure is also one actual execution (the function
                // ran and raised). Blocked steps never ran, so they don't count.
                for (node_id, _) in &failures {
                    let base = crate::StepId::parse(node_id).base_id().to_string();
                    *exec_attempts.entry(base).or_insert(0) += 1;
                }
                Some(crate::scheduler::SchedEvent::UnitFinished {
                    unit,
                    failures,
                    blocked,
                })
            }
            WorkerEvent::ParallelRequest { .. } => {
                // TODO(v0.2.0): Handle parallel dispatch. For now, parallel
                // requests are handled in-band by the reader thread (see
                // spawn_unit). This variant exists for the future async path
                // where the coordinator manages parallel sub-workers.
                None
            }
        }
    }
}

/// Serialize a unit's batch (merging any retry predecessor outputs into the
/// provided map), spawn one Python worker, and start a reader thread that
/// streams `StepCompleted` events and sends one `UnitFinished` on exit.
fn spawn_unit(
    unit: &crate::scheduler::Unit,
    base_provided: &HashMap<String, ProvidedInput>,
    phase_outputs: &HashMap<String, OutputRef>,
    python: &PathBuf,
    tx: &std::sync::mpsc::Sender<WorkerEvent>,
) -> Result<(), String> {
    // Merge base provided inputs with this unit's predecessor outputs (retries).
    let mut provided = base_provided.clone();
    for id in &unit.provided_ids {
        if let Some(oref) = phase_outputs.get(id) {
            provided.insert(id.clone(), ProvidedInput::Single(oref.clone()));
        }
    }

    let ws = WorkerStream {
        stream_id: unit.stream_id.clone(),
        steps: unit.steps.clone(),
    };
    let batch_json = serialize_batch(&ws, &provided);

    let mut batch_file =
        tempfile::NamedTempFile::new().map_err(|e| format!("failed to create temp file: {e}"))?;
    batch_file
        .write_all(batch_json.as_bytes())
        .map_err(|e| format!("failed to write batch: {e}"))?;
    let (_, batch_path) = batch_file
        .keep()
        .map_err(|e| format!("failed to persist temp file: {e}"))?;

    let mut child = match Command::new(python)
        .args(["-m", "barca._worker"])
        .arg(&batch_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .env("BARCA_WORKER", "1")
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            std::fs::remove_file(&batch_path).ok();
            return Err(format!("failed to spawn worker: {e}"));
        }
    };

    let stderr = child.stderr.take().expect("no stderr");
    // Take stdin so the reader thread can use it for the parallel protocol.
    let child_stdin = child.stdin.take().expect("no stdin");
    let tx_reader = tx.clone();
    let unit = unit.clone();
    let python_path = python.clone();

    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut error_lines: Vec<String> = Vec::new();
        let mut failures: Vec<(String, StepError)> = Vec::new();
        let mut blocked: Vec<String> = Vec::new();
        let mut completed: Vec<String> = Vec::new();
        // Kept alive across all parallel_request calls — a worker may issue
        // multiple parallel() calls during its lifetime.
        let mut stdin_pipe = child_stdin;

        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.is_empty() {
                continue;
            }
            if let Some(json_str) = line.strip_prefix(PROTOCOL_PREFIX_V2) {
                let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) else {
                    continue;
                };

                // ── parallel_request protocol ───────────────────────────
                // The worker sends `{"parallel_request": [...]}` (no "type"
                // field) when a task calls `parallel()`. The worker then
                // blocks reading its stdin for the response.
                if let Some(pr) = parsed.get("parallel_request") {
                    let items: Vec<ParallelWorkItem> = match serde_json::from_value(pr.clone()) {
                        Ok(items) => items,
                        Err(e) => {
                            eprintln!("[barca] failed to parse parallel request: {e}");
                            // Send error responses so the worker doesn't hang
                            let count = pr.as_array().map(|a| a.len()).unwrap_or(1);
                            let response: Vec<serde_json::Value> = (0..count)
                                .map(|_| serde_json::json!({"status": "error", "error": format!("failed to parse parallel request: {e}")}))
                                .collect();
                            let response_json = serde_json::to_string(&response)
                                .unwrap_or_else(|_| "[]".to_string());
                            let _ = stdin_pipe.write_all(response_json.as_bytes());
                            let _ = stdin_pipe.write_all(b"\n");
                            let _ = stdin_pipe.flush();
                            continue;
                        }
                    };

                    {
                        // Stage 3: spawn sub-workers for each parallel work
                        // item, collect results, and write the combined
                        // response back to the parent worker's stdin pipe.

                        let artifact_dir = std::env::current_dir()
                            .map(|p| {
                                p.join(".barca")
                                    .join("artifacts")
                                    .to_string_lossy()
                                    .to_string()
                            })
                            .unwrap_or_else(|_| ".barca/artifacts".to_string());

                        // Phase 1: spawn all sub-workers at once for true
                        // parallelism in execution.
                        struct SubWorker {
                            child: std::process::Child,
                            batch_path: std::path::PathBuf,
                        }
                        let mut children: Vec<Result<SubWorker, serde_json::Value>> = Vec::new();

                        for (i, item) in items.iter().enumerate() {
                            // Parse fn_ref into source_file and function_name.
                            let (source_file, function_name) = match item.fn_ref.rsplit_once(':') {
                                Some((file, func)) => (file.to_string(), func.to_string()),
                                None => {
                                    children.push(Err(serde_json::json!({
                                        "status": "error",
                                        "error": format!("invalid fn_ref: {}", item.fn_ref)
                                    })));
                                    continue;
                                }
                            };

                            // Build positional args and keyword args separately.
                            let direct_args: Vec<serde_json::Value> = item.args.clone();
                            let mut direct_kwargs: serde_json::Map<String, serde_json::Value> =
                                serde_json::Map::new();
                            for (k, v) in &item.kwargs {
                                direct_kwargs.insert(k.clone(), v.clone());
                            }

                            // Create a minimal batch JSON.
                            let batch = serde_json::json!({
                                "stream_id": format!("parallel-{i}"),
                                "artifact_dir": &artifact_dir,
                                "steps": [{
                                    "node_id": &item.fn_ref,
                                    "function_name": function_name,
                                    "source_file": source_file,
                                    "kind": "task",
                                    "inputs": {},
                                    "timeout_seconds": 300,
                                    "partition_keys": [],
                                    "serializer": "json",
                                    "direct_args": direct_args,
                                    "direct_kwargs": direct_kwargs,
                                }],
                                "provided_inputs": {}
                            });

                            let batch_json = serde_json::to_string(&batch).unwrap_or_default();
                            let batch_file = match tempfile::NamedTempFile::new() {
                                Ok(mut f) => {
                                    let _ = f.write_all(batch_json.as_bytes());
                                    match f.keep() {
                                        Ok((_, path)) => path,
                                        Err(_) => {
                                            children.push(Err(serde_json::json!({
                                                "status": "error",
                                                "error": "failed to create batch file"
                                            })));
                                            continue;
                                        }
                                    }
                                }
                                Err(_) => {
                                    children.push(Err(serde_json::json!({
                                        "status": "error",
                                        "error": "failed to create temp file"
                                    })));
                                    continue;
                                }
                            };

                            match Command::new(&python_path)
                                .args(["-m", "barca._worker"])
                                .arg(&batch_file)
                                .stdout(Stdio::inherit())
                                .stderr(Stdio::piped())
                                .stdin(Stdio::null())
                                .env("BARCA_WORKER", "1")
                                .spawn()
                            {
                                Ok(child) => {
                                    children.push(Ok(SubWorker {
                                        child,
                                        batch_path: batch_file,
                                    }));
                                }
                                Err(e) => {
                                    std::fs::remove_file(&batch_file).ok();
                                    children.push(Err(serde_json::json!({
                                        "status": "error",
                                        "error": format!("failed to spawn worker: {e}")
                                    })));
                                }
                            }
                        }

                        // Phase 2: collect results from each sub-worker
                        // sequentially. The Python processes are already
                        // running in parallel; we just read each one's stderr
                        // in turn.
                        let mut results: Vec<serde_json::Value> =
                            Vec::with_capacity(children.len());

                        for entry in children {
                            match entry {
                                Err(err_json) => {
                                    results.push(err_json);
                                }
                                Ok(mut sw) => {
                                    let stderr = sw.child.stderr.take().expect("no stderr");
                                    let sub_reader = BufReader::new(stderr);
                                    let mut sub_result: Option<serde_json::Value> = None;
                                    let mut sub_error: Option<String> = None;

                                    for sub_line in sub_reader.lines() {
                                        let Ok(sub_line) = sub_line else { break };
                                        if let Some(json_str) =
                                            sub_line.strip_prefix(PROTOCOL_PREFIX_V2)
                                        {
                                            if let Ok(parsed) =
                                                serde_json::from_str::<serde_json::Value>(json_str)
                                            {
                                                match parsed.get("type").and_then(|v| v.as_str()) {
                                                    Some("result") => {
                                                        if let Some(artifact) =
                                                            parsed.get("artifact")
                                                        {
                                                            let path = artifact
                                                                .get("path")
                                                                .and_then(|v| v.as_str())
                                                                .unwrap_or("");
                                                            let format = artifact
                                                                .get("format")
                                                                .and_then(|v| v.as_str())
                                                                .unwrap_or("json");
                                                            if format == "json" && !path.is_empty()
                                                            {
                                                                if let Ok(content) =
                                                                    std::fs::read_to_string(path)
                                                                {
                                                                    sub_result =
                                                                        serde_json::from_str(
                                                                            &content,
                                                                        )
                                                                        .ok();
                                                                }
                                                            }
                                                            if sub_result.is_none() {
                                                                sub_result = Some(artifact.clone());
                                                            }
                                                        }
                                                    }
                                                    Some("error") => {
                                                        let msg = parsed
                                                            .get("message")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or("unknown error");
                                                        sub_error = Some(msg.to_string());
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }

                                    let _ = sw.child.wait();
                                    std::fs::remove_file(&sw.batch_path).ok();

                                    if let Some(err) = sub_error {
                                        results.push(serde_json::json!({
                                            "status": "error",
                                            "error": err
                                        }));
                                    } else if let Some(val) = sub_result {
                                        results.push(serde_json::json!({
                                            "status": "ok",
                                            "result": val
                                        }));
                                    } else {
                                        results.push(serde_json::json!({
                                            "status": "ok",
                                            "result": null
                                        }));
                                    }
                                }
                            }
                        }

                        // Write combined response to parent's stdin pipe.
                        let response_json =
                            serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string());
                        let _ = stdin_pipe.write_all(response_json.as_bytes());
                        let _ = stdin_pipe.write_all(b"\n");
                        let _ = stdin_pipe.flush();
                    }
                    // Continue reading stderr — the worker resumes after
                    // reading the response and will eventually exit normally.
                    continue;
                }

                match parsed.get("type").and_then(|v| v.as_str()) {
                    Some("result") => {
                        if let (Some(node_id), Some(artifact)) = (
                            parsed.get("node_id").and_then(|v| v.as_str()),
                            parsed.get("artifact"),
                        ) {
                            let oref = OutputRef {
                                path: artifact
                                    .get("path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                format: artifact
                                    .get("format")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                size_bytes: artifact
                                    .get("size_bytes")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                elapsed_seconds: parsed.get("elapsed").and_then(|v| v.as_f64()),
                            };
                            completed.push(node_id.to_string());
                            tx_reader
                                .send(WorkerEvent::StepCompleted {
                                    node_id: node_id.to_string(),
                                    output: oref,
                                })
                                .ok();
                        }
                    }
                    Some("error") => {
                        if let Some(node_id) = parsed.get("node_id").and_then(|v| v.as_str()) {
                            failures.push((node_id.to_string(), parse_step_error(&parsed)));
                        }
                    }
                    Some("blocked") => {
                        if let Some(node_id) = parsed.get("node_id").and_then(|v| v.as_str()) {
                            blocked.push(node_id.to_string());
                        }
                    }
                    _ => {}
                }
            } else if !line.starts_with("BARCA:") {
                error_lines.push(line);
            }
        }

        // Drop the stdin pipe if still held — this signals EOF to the worker
        // if it was somehow waiting.
        drop(stdin_pipe);

        let status = child.wait();
        std::fs::remove_file(&batch_path).ok();

        // Hard-crash fallback: non-zero exit with no structured error reported.
        // Report one WorkerCrash per uncompleted step so the scheduler can
        // apply each step's retry policy and commands.rs can persist them all.
        let crashed = status.map(|s| !s.success()).unwrap_or(true);
        if crashed && failures.is_empty() {
            let traceback = filter_traceback(&error_lines);
            let uncompleted: Vec<String> = unit
                .steps
                .iter()
                .flat_map(|s| {
                    if s.partition_keys.is_empty() {
                        vec![s.step_id.display()]
                    } else {
                        s.partition_keys
                            .iter()
                            .map(|pk| pk.display_id(&s.step_id.base))
                            .collect()
                    }
                })
                .filter(|d| !completed.contains(d))
                .collect();
            if uncompleted.is_empty() {
                failures.push((
                    "<unknown>".to_string(),
                    StepError {
                        error_type: "WorkerCrash".to_string(),
                        message: "worker exited with a non-zero status".to_string(),
                        traceback: traceback.clone(),
                        attempts: 0,
                    },
                ));
            } else {
                for node in uncompleted {
                    failures.push((
                        node,
                        StepError {
                            error_type: "WorkerCrash".to_string(),
                            message: "worker exited with a non-zero status".to_string(),
                            traceback: traceback.clone(),
                            attempts: 0,
                        },
                    ));
                }
            }
        }

        tx_reader
            .send(WorkerEvent::UnitFinished {
                unit,
                failures,
                blocked,
            })
            .ok();
    });

    Ok(())
}

/// Build a `StepError` from an `error` protocol message, filtering the
/// traceback to user-relevant frames.
fn parse_step_error(parsed: &serde_json::Value) -> StepError {
    let raw_tb = parsed
        .get("traceback")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tb_lines: Vec<String> = raw_tb.lines().map(|l| l.to_string()).collect();
    StepError {
        error_type: parsed
            .get("error_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Error")
            .to_string(),
        message: parsed
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        traceback: filter_traceback(&tb_lines),
        attempts: 0,
    }
}

const PROTOCOL_PREFIX_V2: &str = "BARCA:2:";

/// Parse worker stderr output into protocol messages and error lines.
/// Protocol messages are prefixed with `BARCA:2:` followed by a JSON object.
pub fn parse_worker_output(reader: impl BufRead) -> (HashMap<String, OutputRef>, Vec<String>) {
    let mut outputs: HashMap<String, OutputRef> = HashMap::new();
    let mut error_lines: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line.expect("failed to read worker stderr");
        if line.is_empty() {
            continue;
        }
        if let Some(json_str) = line.strip_prefix(PROTOCOL_PREFIX_V2) {
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) else {
                eprintln!("[barca] malformed protocol message: {line}");
                continue;
            };
            match parsed.get("type").and_then(|v| v.as_str()) {
                Some("result") => {
                    if let (Some(node_id), Some(artifact)) = (
                        parsed.get("node_id").and_then(|v| v.as_str()),
                        parsed.get("artifact"),
                    ) {
                        let elapsed = parsed.get("elapsed").and_then(|v| v.as_f64());
                        let oref = OutputRef {
                            path: artifact
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            format: artifact
                                .get("format")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            size_bytes: artifact
                                .get("size_bytes")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                            elapsed_seconds: elapsed,
                        };
                        outputs.insert(node_id.to_string(), oref);
                    }
                }
                Some(_) => {
                    // Unknown message type — ignore (forward-compatible).
                }
                None => {
                    eprintln!("[barca] protocol message missing 'type' field: {line}");
                }
            }
        } else if line.starts_with("BARCA:") {
            eprintln!("[barca] unsupported protocol version: {line}");
        } else {
            error_lines.push(line);
        }
    }

    (outputs, error_lines)
}

/// Serialize a worker stream batch to JSON, including provided inputs and artifact_dir.
pub fn serialize_batch(
    stream: &WorkerStream,
    provided_inputs: &HashMap<String, ProvidedInput>,
) -> String {
    let artifact_dir = ".barca/artifacts";

    let steps: Vec<serde_json::Value> = stream
        .steps
        .iter()
        .map(|s| {
            // With late expansion, inputs are always base (unpartitioned) IDs.
            // The worker resolves partition-aligned inputs when iterating partition_keys.
            let inputs = s.inputs.clone();

            let mut step = serde_json::json!({
                "node_id": s.step_id.display(),
                "kind": s.kind,
                "function_name": s.function_name,
                "source_file": s.source_file,
                "inputs": inputs,
            });
            if !s.partition_keys.is_empty() {
                // Serialize partition_keys as array of objects: [{"ticker":"AAPL"}, ...]
                let pks: Vec<serde_json::Value> = s
                    .partition_keys
                    .iter()
                    .map(|pk| serde_json::json!(pk.0))
                    .collect();
                step["partition_keys"] = serde_json::json!(pks);
            }
            if let Some(ref ser) = s.serializer {
                step["serializer"] = serde_json::json!(ser);
            }
            if s.timeout_seconds > 0 {
                step["timeout_seconds"] = serde_json::json!(s.timeout_seconds);
            }
            step
        })
        .collect();

    // Serialize provided_inputs — single artifacts or collected lists.
    let pi_json: HashMap<String, serde_json::Value> = provided_inputs
        .iter()
        .map(|(k, pi)| {
            let val = match pi {
                ProvidedInput::Single(oref) => serde_json::json!({
                    "path": oref.path,
                    "format": oref.format,
                    "size_bytes": oref.size_bytes,
                }),
                ProvidedInput::Collected(orefs) => serde_json::json!({
                    "_collected": true,
                    "artifacts": orefs.iter().map(|o| serde_json::json!({
                        "path": o.path,
                        "format": o.format,
                    })).collect::<Vec<_>>(),
                }),
            };
            (k.clone(), val)
        })
        .collect();

    serde_json::json!({
        "stream_id": stream.stream_id,
        "artifact_dir": artifact_dir,
        "provided_inputs": pi_json,
        "steps": steps,
    })
    .to_string()
}

/// Filter a Python traceback to show only user-relevant frames.
/// Handles chained exceptions (`raise X from Y`, "During handling...").
/// Uses path component matching (not substring) to identify barca internals.
fn filter_traceback(lines: &[String]) -> String {
    let mut result: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("Traceback (most recent call last):") {
            result.push("Traceback (most recent call last):");
            i += 1;
            // Filter frames within this traceback block.
            while i < lines.len() {
                let trimmed = lines[i].trim();
                if trimmed.starts_with("File \"") {
                    if is_internal_frame(trimmed) {
                        // Skip internal frame + its code/underline lines.
                        i += 1;
                        while i < lines.len() {
                            let t = lines[i].trim();
                            if t.starts_with("File \"") || is_exception_line(t, &lines[i]) {
                                break;
                            }
                            i += 1;
                        }
                    } else {
                        // Keep user frame + its code/underline lines.
                        result.push(&lines[i]);
                        i += 1;
                        while i < lines.len() {
                            let t = lines[i].trim();
                            if t.starts_with("File \"") || is_exception_line(t, &lines[i]) {
                                break;
                            }
                            result.push(&lines[i]);
                            i += 1;
                        }
                    }
                } else {
                    // Exception line or chained traceback header — keep it.
                    result.push(&lines[i]);
                    i += 1;
                    // If this is a chain header, the next iteration handles the new Traceback.
                    break;
                }
            }
        } else {
            // Non-traceback line (chain separator, etc.) — keep it.
            result.push(&lines[i]);
            i += 1;
        }
    }

    result.join("\n")
}

/// Check if a `File "..."` frame line is a barca internal frame.
/// Uses path components, not substring matching, to avoid false positives
/// on user files that happen to contain "barca/_worker.py" in their path.
fn is_internal_frame(trimmed: &str) -> bool {
    // Extract the path from `File "path", line N, in func`
    let path = trimmed
        .strip_prefix("File \"")
        .and_then(|s| s.split('"').next())
        .unwrap_or("");

    // Check for frozen internals.
    if path.starts_with("<frozen ") {
        return true;
    }

    // Check path components: last two components should be "barca/_worker.py" etc.
    let parts: Vec<&str> = path.rsplit('/').take(2).collect();
    if parts.len() == 2 {
        let file = parts[0];
        let parent = parts[1];
        if parent == "barca" && (file == "_worker.py" || file == "_runner.py") {
            return true;
        }
    }
    // Also check Windows paths.
    let parts: Vec<&str> = path.rsplit('\\').take(2).collect();
    if parts.len() == 2 {
        let file = parts[0];
        let parent = parts[1];
        if parent == "barca" && (file == "_worker.py" || file == "_runner.py") {
            return true;
        }
    }

    false
}

/// Check if a line is an exception line (not a frame or code line).
fn is_exception_line(trimmed: &str, raw: &str) -> bool {
    !trimmed.is_empty()
        && !trimmed.starts_with("File \"")
        && !trimmed.starts_with('~')
        && !raw.starts_with("    ")
        && !raw.starts_with('\t')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::PhaseReason;
    use std::sync::Arc;

    #[test]
    fn parse_worker_output_v2_separates_protocol_from_errors() {
        let input = "BARCA:2:{\"type\":\"result\",\"node_id\":\"test.py:foo\",\"artifact\":{\"path\":\"foo.json\",\"format\":\"json\",\"size_bytes\":10},\"elapsed\":0.01}\n\
some error message\n\
BARCA:2:{\"type\":\"result\",\"node_id\":\"test.py:bar\",\"artifact\":{\"path\":\"bar.json\",\"format\":\"json\",\"size_bytes\":20},\"elapsed\":0.02}\n\
Traceback (most recent call last):\n\
  File \"test.py\", line 5\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs["test.py:foo"].format, "json");
        assert_eq!(outputs["test.py:bar"].format, "json");
        assert_eq!(errors.len(), 3);
        assert!(errors[0].contains("some error message"));
    }

    #[test]
    fn parse_worker_output_ignores_empty_lines() {
        let input = "\n\nBARCA:2:{\"type\":\"result\",\"node_id\":\"a\",\"artifact\":{\"path\":\"a.json\",\"format\":\"json\",\"size_bytes\":5}}\n\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert_eq!(outputs.len(), 1);
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_worker_output_bare_json_is_error_line() {
        // JSON that looks like a protocol message but lacks the BARCA: prefix
        // should NOT be parsed as protocol — it's an error line.
        let input = "{\"node_id\": \"fake\", \"output\": \"injected\"}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert!(outputs.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn parse_worker_output_unknown_type_ignored() {
        let input = "BARCA:2:{\"type\":\"progress\",\"node_id\":\"a\",\"pct\":50}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert!(outputs.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_worker_output_unsupported_version() {
        let input = "BARCA:99:{\"type\":\"result\",\"node_id\":\"a\",\"output\":1}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert!(outputs.is_empty());
        assert!(errors.is_empty()); // version mismatch is logged, not an error line
    }

    #[test]
    fn parse_worker_output_v1_is_unsupported() {
        // v1 messages should now be treated as unsupported version, not parsed.
        let input = "BARCA:1:{\"type\":\"result\",\"node_id\":\"a\",\"output\":1}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert!(outputs.is_empty());
        assert!(errors.is_empty());
    }

    use crate::{NodeKind, PartitionKey, StepId};

    fn test_output_ref(path: &str, format: &str) -> OutputRef {
        OutputRef {
            path: path.to_string(),
            format: format.to_string(),
            size_bytes: 100,
            elapsed_seconds: None,
        }
    }

    #[test]
    fn build_provided_inputs_direct_match() {
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:b"),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("b"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::from([("a_val".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };
        let mut all_outputs = HashMap::new();
        all_outputs.insert("f:a".to_string(), test_output_ref("f--a.json", "json"));

        let provided = build_provided_inputs(&phase, &all_outputs);
        match &provided["f:a"] {
            ProvidedInput::Single(oref) => assert_eq!(oref.path, "f--a.json"),
            other => panic!("expected Single, got {other:?}"),
        }
    }

    #[test]
    fn build_provided_inputs_partition_aligned() {
        let pk = PartitionKey::from(HashMap::from([("t".to_string(), "X".to_string())]));
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::new("f:b", pk),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("b"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::from([("a_val".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };
        let mut all_outputs = HashMap::new();
        all_outputs.insert(
            "f:a[t=X]".to_string(),
            test_output_ref("f--a_t_X.json", "json"),
        );

        let provided = build_provided_inputs(&phase, &all_outputs);
        match &provided["f:a[t=X]"] {
            ProvidedInput::Single(oref) => assert_eq!(oref.path, "f--a_t_X.json"),
            other => panic!("expected Single, got {other:?}"),
        }
    }

    #[test]
    fn expand_pending_partitions_no_pending_returns_none() {
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:a"),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("a"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };
        assert!(
            expand_pending_partitions(&phase, &HashMap::<String, OutputRef>::new(), 4).is_none()
        );
    }

    #[test]
    fn expand_pending_partitions_expands_derived() {
        // Create a temporary JSON artifact file containing partition values.
        let dir = tempfile::tempdir().unwrap();
        let artifact_path = dir.path().join("regions.json");
        std::fs::write(&artifact_path, r#"["us","eu"]"#).unwrap();

        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:transform"),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("transform"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::from([(
                        "region".to_string(),
                        "get_regions".to_string(),
                    )]),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };
        let mut outputs = HashMap::new();
        outputs.insert(
            "f:get_regions".to_string(),
            OutputRef {
                path: artifact_path.to_string_lossy().to_string(),
                format: "json".to_string(),
                size_bytes: 12,
                elapsed_seconds: None,
            },
        );

        let expanded = expand_pending_partitions(&phase, &outputs, 4).unwrap();
        // Late expansion: expanded steps have partition_keys, not individual step_ids.
        let all_pks: Vec<String> = expanded
            .streams
            .iter()
            .flat_map(|s| &s.steps)
            .flat_map(|st| {
                st.partition_keys
                    .iter()
                    .map(|pk| pk.display_id(&st.step_id.base))
            })
            .collect();
        assert!(all_pks.contains(&"f:transform[region=eu]".to_string()));
        assert!(all_pks.contains(&"f:transform[region=us]".to_string()));
        assert_eq!(all_pks.len(), 2);
    }

    #[test]
    fn serialize_batch_includes_partition_keys() {
        // Late expansion: step has unpartitioned step_id + partition_keys.
        // Inputs are base IDs — the worker resolves partition-aligned inputs.
        let pk = PartitionKey::from(HashMap::from([("t".to_string(), "X".to_string())]));
        let stream = WorkerStream {
            stream_id: "w0".to_string(),
            steps: vec![StreamStep {
                step_id: StepId::unpartitioned("f:b"),
                kind: NodeKind::Asset,
                function_name: Arc::from("b"),
                source_file: Arc::from("f"),
                inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                pending_partitions: HashMap::new(),
                serializer: None,
                timeout_seconds: 300,
                retries: 1,
                retry_backoff_seconds: 0.0,
                partition_keys: vec![pk],
            }],
        };

        let json_str = serialize_batch(&stream, &HashMap::<String, ProvidedInput>::new());
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        // Inputs are base IDs (worker handles alignment)
        assert_eq!(parsed["steps"][0]["inputs"]["data"], "f:a");
        // partition_keys should be serialized as array of objects
        let pks = &parsed["steps"][0]["partition_keys"];
        assert_eq!(pks.as_array().unwrap().len(), 1);
        assert_eq!(pks[0]["t"], "X");
    }

    // ─── v2 protocol + OutputRef tests ───────────────────────────────────────

    #[test]
    fn parse_v2_result_with_artifact() {
        let input = "BARCA:2:{\"type\":\"result\",\"node_id\":\"test.py:foo\",\"artifact\":{\"path\":\".barca/artifacts/test.py--foo.json\",\"format\":\"json\",\"size_bytes\":42},\"elapsed\":0.01}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);

        assert_eq!(outputs.len(), 1);
        let output = &outputs["test.py:foo"];
        assert_eq!(
            *output,
            OutputRef {
                path: ".barca/artifacts/test.py--foo.json".to_string(),
                format: "json".to_string(),
                size_bytes: 42,
                elapsed_seconds: Some(0.01),
            }
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_v2_mixed_with_errors() {
        let input = "BARCA:2:{\"type\":\"result\",\"node_id\":\"a\",\"artifact\":{\"path\":\"a.json\",\"format\":\"json\",\"size_bytes\":10}}\n\
            some error line\n\
            BARCA:2:{\"type\":\"result\",\"node_id\":\"b\",\"artifact\":{\"path\":\"b.pkl\",\"format\":\"pickle\",\"size_bytes\":200}}\n\
            Traceback:\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs["a"].format, "json");
        assert_eq!(outputs["b"].format, "pickle");
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn parse_v2_malformed_skipped() {
        let input = "BARCA:2:not-json\n\
            BARCA:2:{\"type\":\"result\",\"node_id\":\"a\",\"artifact\":{\"path\":\"a.json\",\"format\":\"json\",\"size_bytes\":10}}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert_eq!(outputs.len(), 1);
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_v2_unknown_type_ignored() {
        let input = "BARCA:2:{\"type\":\"progress\",\"pct\":50}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);
        assert!(outputs.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn parse_v2_parquet_artifact() {
        let input = "BARCA:2:{\"type\":\"result\",\"node_id\":\"pipeline.py:df\",\"artifact\":{\"path\":\".barca/artifacts/pipeline.py--df.parquet\",\"format\":\"parquet\",\"size_bytes\":8192},\"elapsed\":0.5}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, _) = parse_worker_output(reader);

        assert_eq!(outputs.len(), 1);
        let output = &outputs["pipeline.py:df"];
        assert_eq!(output.format, "parquet");
        assert_eq!(output.size_bytes, 8192);
    }

    #[test]
    fn parse_v2_pickle_artifact() {
        let input = "BARCA:2:{\"type\":\"result\",\"node_id\":\"m.py:obj\",\"artifact\":{\"path\":\".barca/artifacts/m.py--obj.pkl\",\"format\":\"pickle\",\"size_bytes\":512},\"elapsed\":0.02}\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, _) = parse_worker_output(reader);

        assert_eq!(outputs["m.py:obj"].format, "pickle");
        assert_eq!(outputs["m.py:obj"].size_bytes, 512);
    }

    #[test]
    fn build_provided_inputs_with_output_ref_direct() {
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:b"),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("b"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };
        let mut all_outputs: HashMap<String, OutputRef> = HashMap::new();
        all_outputs.insert(
            "f:a".to_string(),
            OutputRef {
                path: ".barca/artifacts/f--a.json".to_string(),
                format: "json".to_string(),
                size_bytes: 100,
                elapsed_seconds: None,
            },
        );

        let provided = build_provided_inputs(&phase, &all_outputs);
        match &provided["f:a"] {
            ProvidedInput::Single(oref) => assert_eq!(oref.path, ".barca/artifacts/f--a.json"),
            _ => panic!("expected Single"),
        }
    }

    #[test]
    fn build_provided_inputs_with_output_ref_partition_aligned() {
        let pk = PartitionKey::from(HashMap::from([("t".to_string(), "X".to_string())]));
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::new("f:b", pk),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("b"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };
        let mut all_outputs: HashMap<String, OutputRef> = HashMap::new();
        all_outputs.insert(
            "f:a[t=X]".to_string(),
            OutputRef {
                path: ".barca/artifacts/f--a_t_X.parquet".to_string(),
                format: "parquet".to_string(),
                size_bytes: 5000,
                elapsed_seconds: None,
            },
        );

        let provided = build_provided_inputs(&phase, &all_outputs);
        match &provided["f:a[t=X]"] {
            ProvidedInput::Single(oref) => assert_eq!(oref.format, "parquet"),
            _ => panic!("expected Single"),
        }
    }

    #[test]
    fn serialize_batch_with_output_ref_provided_inputs() {
        let stream = WorkerStream {
            stream_id: "w0".to_string(),
            steps: vec![StreamStep {
                step_id: StepId::unpartitioned("f:b"),
                kind: NodeKind::Asset,
                function_name: Arc::from("b"),
                source_file: Arc::from("f"),
                inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                pending_partitions: HashMap::new(),
                serializer: None,
                timeout_seconds: 300,
                retries: 1,
                retry_backoff_seconds: 0.0,
                partition_keys: vec![],
            }],
        };
        let mut provided: HashMap<String, ProvidedInput> = HashMap::new();
        provided.insert(
            "f:a".to_string(),
            ProvidedInput::Single(OutputRef {
                path: ".barca/artifacts/f--a.json".to_string(),
                format: "json".to_string(),
                size_bytes: 100,
                elapsed_seconds: None,
            }),
        );

        let json_str = serialize_batch(&stream, &provided);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        // provided_inputs should serialize OutputRef as object with path/format
        let pi = &parsed["provided_inputs"]["f:a"];
        assert_eq!(pi["path"], ".barca/artifacts/f--a.json");
        assert_eq!(pi["format"], "json");
        assert_eq!(pi["size_bytes"], 100);
    }

    #[test]
    fn serialize_batch_includes_artifact_dir() {
        let stream = WorkerStream {
            stream_id: "w0".to_string(),
            steps: vec![StreamStep {
                step_id: StepId::unpartitioned("f:a"),
                kind: NodeKind::Asset,
                function_name: Arc::from("a"),
                source_file: Arc::from("f"),
                inputs: HashMap::new(),
                pending_partitions: HashMap::new(),
                serializer: None,
                timeout_seconds: 300,
                retries: 1,
                retry_backoff_seconds: 0.0,
                partition_keys: vec![],
            }],
        };

        let json_str = serialize_batch(&stream, &HashMap::<String, ProvidedInput>::new());
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        // artifact_dir should be present in the batch JSON
        assert!(parsed.get("artifact_dir").is_some());
    }

    #[test]
    fn expand_pending_partitions_reads_json_artifact() {
        // Create a temporary JSON artifact file containing partition values.
        let dir = tempfile::tempdir().unwrap();
        let artifact_path = dir.path().join("regions.json");
        std::fs::write(&artifact_path, r#"["us","eu","ap"]"#).unwrap();

        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:transform"),
                    kind: NodeKind::Asset,
                    function_name: Arc::from("transform"),
                    source_file: Arc::from("f"),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::from([(
                        "region".to_string(),
                        "get_regions".to_string(),
                    )]),
                    serializer: None,
                    timeout_seconds: 300,
                    retries: 1,
                    retry_backoff_seconds: 0.0,
                    partition_keys: vec![],
                }],
            }],
        };

        let mut outputs: HashMap<String, OutputRef> = HashMap::new();
        outputs.insert(
            "f:get_regions".to_string(),
            OutputRef {
                path: artifact_path.to_string_lossy().to_string(),
                format: "json".to_string(),
                size_bytes: 14,
                elapsed_seconds: None,
            },
        );

        let expanded = expand_pending_partitions(&phase, &outputs, 4).unwrap();
        // Late expansion: expanded steps have partition_keys, not individual step_ids.
        let all_pks: Vec<String> = expanded
            .streams
            .iter()
            .flat_map(|s| &s.steps)
            .flat_map(|st| {
                st.partition_keys
                    .iter()
                    .map(|pk| pk.display_id(&st.step_id.base))
            })
            .collect();
        assert!(all_pks.contains(&"f:transform[region=us]".to_string()));
        assert!(all_pks.contains(&"f:transform[region=eu]".to_string()));
        assert!(all_pks.contains(&"f:transform[region=ap]".to_string()));
        assert_eq!(all_pks.len(), 3);
    }

    #[test]
    fn parse_step_error_filters_internal_frames() {
        let parsed: serde_json::Value = serde_json::json!({
            "type": "error",
            "node_id": "f:boom",
            "error_type": "ValueError",
            "message": "kaboom",
            "traceback": "Traceback (most recent call last):\n  \
                File \"/x/barca/_worker.py\", line 1, in run\n    fn()\n  \
                File \"user.py\", line 3, in boom\n    raise ValueError('kaboom')\n\
                ValueError: kaboom",
        });
        let err = parse_step_error(&parsed);
        assert_eq!(err.error_type, "ValueError");
        assert_eq!(err.message, "kaboom");
        assert_eq!(err.attempts, 0); // filled in by the scheduler when permanent
        // User frame retained, internal _worker.py frame stripped.
        assert!(err.traceback.contains("user.py"));
        assert!(!err.traceback.contains("_worker.py"));
        assert!(err.traceback.contains("ValueError: kaboom"));
    }
}
