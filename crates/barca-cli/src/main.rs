//! Barca CLI — multi-process phased execution.
//!
//! Flow: parse → DAG → planner → dispatch phases → collect stdout → persist to Turso

use barca_core::dag::Dag;
use barca_core::parse::extract_nodes;
use barca_core::planner::ResourceConfig;
use barca_core::planner::{self, ExecutionPlan, Phase, WorkerStream};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;
use turso::Builder;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: barca run <python_file> [python_file ...]");
        std::process::exit(1);
    }

    let command = &args[1];
    match command.as_str() {
        "run" => run_command(&args[2..]),
        "plan" => plan_command(&args[2..]),
        "--help" | "-h" => {
            println!("barca — invisible asset orchestrator");
            println!();
            println!("Commands:");
            println!("  run <file> [file ...]   Parse, plan, and execute assets");
            println!("  plan <file> [file ...]  Parse and emit execution plan (JSON)");
        }
        _ => {
            run_command(&args[1..]);
        }
    }
}

fn find_python() -> PathBuf {
    let self_exe = env::current_exe().expect("cannot determine own path");
    let bin_dir = self_exe.parent().expect("binary has no parent dir");
    bin_dir.join("python")
}

fn run_command(file_args: &[String]) {
    let t0 = Instant::now();
    let python = find_python();

    let dag = build_dag(file_args, &python);
    let node_count = dag.node_count();
    let edge_count = dag.edge_count();

    let pool_size = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let config = ResourceConfig {
        pool_size,
        concurrency_groups: HashMap::new(),
    };
    let exec_plan = planner::plan_from_dag(&dag, &config);
    let plan_time = t0.elapsed();

    // Initialize DB.
    let db_path = ensure_db_dir();
    init_db_sync(&db_path);

    let t_exec = Instant::now();
    let all_outputs = dispatch_plan(&exec_plan, &python, &db_path, pool_size);
    let exec_time = t_exec.elapsed();

    // Persist all outputs to Turso.
    persist_outputs_sync(&db_path, &all_outputs);

    // Print final result — find the last step from the last phase.
    // For expanded partitions, match any output whose node_id starts with the planned step's id.
    let total_executed = all_outputs.len();
    let last_planned_id = exec_plan
        .phases
        .last()
        .and_then(|p| p.streams.last())
        .and_then(|s| s.steps.last())
        .map(|s| s.node_id.as_str())
        .unwrap_or("");
    let last_output = all_outputs.get(last_planned_id).or_else(|| {
        // Try prefix match for expanded partition steps.
        all_outputs
            .iter()
            .filter(|(k, _)| k.starts_with(last_planned_id))
            .map(|(_, v)| v)
            .last()
    });
    if let Some(output) = last_output {
        println!(
            "{}",
            serde_json::json!({
                "elapsed_seconds": exec_time.as_secs_f64(),
                "steps_executed": total_executed,
                "phases": exec_plan.phases.len(),
                "final_output": output,
            })
        );
    }

    let total_time = t0.elapsed();
    eprintln!(
        "[barca] {} nodes, {} edges, {} phases, {} streams | plan: {:?} | exec: {:?} | total: {:?}",
        node_count,
        edge_count,
        exec_plan.phases.len(),
        exec_plan
            .phases
            .iter()
            .map(|p| p.streams.len())
            .sum::<usize>(),
        plan_time,
        exec_time,
        total_time
    );
}

/// Dispatch all phases, collecting outputs. Returns node_id → output value.
fn dispatch_plan(
    plan: &ExecutionPlan,
    python: &PathBuf,
    _db_path: &str,
    pool_size: usize,
) -> HashMap<String, serde_json::Value> {
    let mut all_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    for phase in &plan.phases {
        // Expand any pending_partitions using outputs from prior phases.
        let expanded_phase = expand_pending_partitions(phase, &all_outputs, pool_size);
        let phase_ref = expanded_phase.as_ref().unwrap_or(phase);

        // Build provided_inputs for this phase.
        let provided = build_provided_inputs(phase_ref, &all_outputs);

        // Spawn workers, collect their stdout.
        let phase_outputs = execute_phase(phase_ref, &provided, python);

        // Merge into global outputs.
        for (node_id, value) in phase_outputs {
            all_outputs.insert(node_id, value);
        }
    }

    all_outputs
}

/// Expand steps with pending_partitions using materialized source outputs.
/// Returns None if no expansion needed, or a new Phase with expanded steps.
fn expand_pending_partitions(
    phase: &Phase,
    all_outputs: &HashMap<String, serde_json::Value>,
    pool_size: usize,
) -> Option<Phase> {
    use barca_core::planner::WorkerStream;

    let has_pending = phase
        .streams
        .iter()
        .any(|s| s.steps.iter().any(|st| !st.pending_partitions.is_empty()));

    if !has_pending {
        return None;
    }

    // Expand all pending steps.
    let mut all_expanded_steps: Vec<barca_core::planner::StreamStep> = Vec::new();
    let mut passthrough_steps: Vec<barca_core::planner::StreamStep> = Vec::new();

    for stream in &phase.streams {
        for step in &stream.steps {
            if step.pending_partitions.is_empty() {
                passthrough_steps.push(step.clone());
                continue;
            }

            // For each pending dimension, look up the source output.
            let mut dim_values: HashMap<String, Vec<String>> = HashMap::new();
            for (dim, source_name) in &step.pending_partitions {
                // Find the source node's output (try exact match, then search by function name).
                let source_output = all_outputs
                    .iter()
                    .find(|(k, _)| k.ends_with(&format!(":{source_name}")))
                    .map(|(_, v)| v);

                if let Some(output) = source_output {
                    // Extract partition values from the output (expect a JSON array).
                    let values: Vec<String> = match output {
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

            // Expand into N steps (cartesian product of all dimensions).
            let combos = expand_combos(&dim_values);
            for combo in combos {
                let suffix = combo
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                all_expanded_steps.push(barca_core::planner::StreamStep {
                    node_id: format!("{}[{suffix}]", step.node_id),
                    function_name: step.function_name.clone(),
                    source_file: step.source_file.clone(),
                    inputs: step.inputs.clone(),
                    partition: combo,
                    pending_partitions: HashMap::new(),
                });
            }
        }
    }

    // Distribute expanded steps across streams.
    let total_steps = all_expanded_steps.len() + passthrough_steps.len();
    let num_streams = total_steps.min(pool_size).max(1);
    let mut streams: Vec<Vec<barca_core::planner::StreamStep>> = vec![Vec::new(); num_streams];
    let mut sizes: Vec<usize> = vec![0; num_streams];

    // Passthrough steps first.
    for step in passthrough_steps {
        let target = sizes
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| *s)
            .map(|(i, _)| i)
            .unwrap_or(0);
        sizes[target] += 1;
        streams[target].push(step);
    }
    // Then expanded partition steps.
    for step in all_expanded_steps {
        let target = sizes
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| *s)
            .map(|(i, _)| i)
            .unwrap_or(0);
        sizes[target] += 1;
        streams[target].push(step);
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

/// Expand partition dimension values into cartesian product combinations.
fn expand_combos(dims: &HashMap<String, Vec<String>>) -> Vec<HashMap<String, String>> {
    let mut combos = vec![HashMap::new()];
    for (dim, values) in dims {
        let mut new_combos = Vec::new();
        for combo in &combos {
            for val in values {
                let mut c = combo.clone();
                c.insert(dim.clone(), val.clone());
                new_combos.push(c);
            }
        }
        combos = new_combos;
    }
    combos
}

/// Determine what values need to be provided to workers in this phase.
fn build_provided_inputs(
    phase: &Phase,
    all_outputs: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut provided: HashMap<String, serde_json::Value> = HashMap::new();

    for stream in &phase.streams {
        for step in &stream.steps {
            for upstream_id in step.inputs.values() {
                if all_outputs.contains_key(upstream_id) && !provided.contains_key(upstream_id) {
                    provided.insert(upstream_id.clone(), all_outputs[upstream_id].clone());
                }
            }
        }
    }

    provided
}

/// Execute a single phase: spawn N workers in parallel, collect results from stderr.
/// User stdout passes through to the terminal.
fn execute_phase(
    phase: &Phase,
    provided_inputs: &HashMap<String, serde_json::Value>,
    python: &PathBuf,
) -> HashMap<String, serde_json::Value> {
    let mut children: Vec<(std::process::Child, PathBuf)> = Vec::new();

    for stream in &phase.streams {
        let batch_json = serialize_batch(stream, provided_inputs);
        let mut batch_file = tempfile::NamedTempFile::new().expect("failed to create temp file");
        batch_file
            .write_all(batch_json.as_bytes())
            .expect("failed to write batch");
        let (_, path) = batch_file.keep().expect("failed to persist temp file");

        let child = Command::new(python)
            .args(["-m", "barca._worker"])
            .arg(&path)
            .stdout(Stdio::inherit()) // User prints pass through to terminal
            .stderr(Stdio::piped()) // Protocol messages (JSON lines) on stderr
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn worker: {e}"));

        children.push((child, path));
    }

    // Collect protocol outputs from all workers' stderr.
    let mut phase_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    for (mut child, batch_path) in children {
        let stderr = child.stderr.take().expect("no stderr");
        let reader = BufReader::new(stderr);

        // Separate protocol lines (JSON) from error messages.
        let mut error_lines: Vec<String> = Vec::new();
        for line in reader.lines() {
            let line = line.expect("failed to read worker stderr");
            if line.is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line)
                && let (Some(node_id), Some(output)) = (
                    parsed.get("node_id").and_then(|v| v.as_str()),
                    parsed.get("output"),
                )
            {
                phase_outputs.insert(node_id.to_string(), output.clone());
            } else {
                // Not a protocol message — it's an actual error/traceback.
                error_lines.push(line);
            }
        }

        let status = child.wait().expect("failed to wait on worker");
        if !status.success() {
            eprintln!("[barca] Worker failed:\n{}", error_lines.join("\n"));
            std::fs::remove_file(&batch_path).ok();
            std::process::exit(1);
        }
        std::fs::remove_file(&batch_path).ok();
    }

    phase_outputs
}

/// Serialize a worker stream batch to JSON, including provided inputs.
fn serialize_batch(
    stream: &WorkerStream,
    provided_inputs: &HashMap<String, serde_json::Value>,
) -> String {
    let steps: Vec<serde_json::Value> = stream
        .steps
        .iter()
        .map(|s| {
            let mut step = serde_json::json!({
                "node_id": s.node_id,
                "function_name": s.function_name,
                "source_file": s.source_file,
                "inputs": s.inputs,
            });
            if !s.partition.is_empty() {
                step["partition"] = serde_json::json!(s.partition);
            }
            step
        })
        .collect();

    serde_json::json!({
        "stream_id": stream.stream_id,
        "provided_inputs": provided_inputs,
        "steps": steps,
    })
    .to_string()
}

fn plan_command(file_args: &[String]) {
    let python = find_python();
    let dag = build_dag(file_args, &python);
    let config = ResourceConfig {
        pool_size: 10,
        concurrency_groups: HashMap::new(),
    };
    let plan = planner::plan_from_dag(&dag, &config);
    let phases: Vec<serde_json::Value> = plan
        .phases
        .iter()
        .map(|p| {
            let streams: Vec<serde_json::Value> = p
                .streams
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "stream_id": s.stream_id,
                        "steps": s.steps.iter().map(|st| &st.node_id).collect::<Vec<_>>(),
                    })
                })
                .collect();
            serde_json::json!({
                "reason": format!("{:?}", p.reason),
                "streams": streams,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "total_steps": plan.total_steps,
            "phases": phases,
        }))
        .unwrap()
    );
}

fn build_dag(file_args: &[String], python: &PathBuf) -> Dag {
    let paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();
    let mut all_nodes = Vec::new();
    for path in &paths {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Error reading {}: {e}", path.display()));
        let file_str = path.to_string_lossy().to_string();
        let nodes = extract_nodes(&source, &file_str).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        all_nodes.extend(nodes);
    }

    // Resolve Dynamic partition specs by spawning Python to evaluate expressions.
    resolve_dynamic_partitions(&mut all_nodes, python);

    Dag::build(&all_nodes).unwrap_or_else(|e| {
        eprintln!("DAG error: {e}");
        std::process::exit(1);
    })
}

/// Resolve PartitionSpec::Dynamic by evaluating Python expressions.
/// Mutates nodes in place: Dynamic → Static.
fn resolve_dynamic_partitions(nodes: &mut [barca_core::model::ExtractedNode], python: &PathBuf) {
    for node in nodes.iter_mut() {
        let mut resolved: Vec<(String, Vec<barca_core::model::PartitionValue>)> = Vec::new();

        for (dim, spec) in &node.partitions {
            if let barca_core::model::PartitionSpec::Dynamic { source_text } = spec {
                // Spawn Python to evaluate the expression within the user's module context.
                let module_path = std::path::Path::new(&node.source_file)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&node.source_file));
                let script = format!(
                    "import json, importlib.util; \
                     _spec = importlib.util.spec_from_file_location('_m', '{path}'); \
                     _mod = importlib.util.module_from_spec(_spec); \
                     _spec.loader.exec_module(_mod); \
                     _ns = vars(_mod); _ns['__builtins__'] = __builtins__; \
                     print(json.dumps(eval('{expr}', _ns)))",
                    path = module_path.display(),
                    expr = source_text.replace('\'', "\\'"),
                );
                let output = Command::new(python)
                    .args(["-c", &script])
                    .output()
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to evaluate partition expression for {}: {e}",
                            node.function_name
                        )
                    });

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!(
                        "Warning: failed to evaluate partition expression '{}' for {}: {}",
                        source_text,
                        node.function_name,
                        stderr.trim()
                    );
                    continue;
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let values: Vec<serde_json::Value> =
                    serde_json::from_str(stdout.trim()).unwrap_or_default();
                let partition_values: Vec<barca_core::model::PartitionValue> = values
                    .into_iter()
                    .filter_map(|v| match v {
                        serde_json::Value::String(s) => {
                            Some(barca_core::model::PartitionValue::Str(s))
                        }
                        serde_json::Value::Number(n) => {
                            n.as_i64().map(barca_core::model::PartitionValue::Int)
                        }
                        _ => None,
                    })
                    .collect();

                resolved.push((dim.clone(), partition_values));
            }
        }

        // Replace Dynamic specs with Static.
        for (dim, values) in resolved {
            node.partitions
                .insert(dim, barca_core::model::PartitionSpec::Static { values });
        }
    }
}

fn ensure_db_dir() -> String {
    let db_dir = PathBuf::from(".barca");
    fs::create_dir_all(&db_dir).ok();
    db_dir.join("metadata.db").to_string_lossy().to_string()
}

fn init_db_sync(db_path: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS materializations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                node_id TEXT NOT NULL,
                output_json TEXT,
                elapsed_seconds REAL,
                status TEXT NOT NULL DEFAULT 'success',
                created_at TEXT DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mat_node_id ON materializations(node_id)",
            (),
        )
        .await
        .unwrap();
    });
}

fn persist_outputs_sync(db_path: &str, outputs: &HashMap<String, serde_json::Value>) {
    if outputs.is_empty() {
        return;
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        for (node_id, output) in outputs {
            let output_json = serde_json::to_string(output).unwrap_or_default();
            conn.execute(
                "INSERT INTO materializations (node_id, output_json) VALUES (?1, ?2)",
                [node_id.clone(), output_json],
            )
            .await
            .ok();
        }
    });
}
