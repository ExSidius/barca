//! Barca CLI — multi-process phased execution.
//!
//! Flow: parse → DAG → planner → dispatch phases → collect stdout → persist to Turso

use barca_core::dag::Dag;
use barca_core::parse::extract_nodes;
use barca_core::plan::ResourceConfig;
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

fn run_command(file_args: &[String]) {
    let t0 = Instant::now();

    let dag = build_dag(file_args);
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

    // Dispatch and collect.
    let self_exe = env::current_exe().expect("cannot determine own path");
    let bin_dir = self_exe.parent().expect("binary has no parent dir");
    let python = bin_dir.join("python");

    let t_exec = Instant::now();
    let all_outputs = dispatch_plan(&exec_plan, &python, &db_path);
    let exec_time = t_exec.elapsed();

    // Persist all outputs to Turso.
    persist_outputs_sync(&db_path, &all_outputs);

    // Print final result.
    let last_node_id = exec_plan
        .phases
        .last()
        .and_then(|p| p.streams.last())
        .and_then(|s| s.steps.last())
        .map(|s| s.node_id.as_str());

    if let Some(node_id) = last_node_id
        && let Some(output) = all_outputs.get(node_id)
    {
        println!(
            "{}",
            serde_json::json!({
                "elapsed_seconds": exec_time.as_secs_f64(),
                "steps_executed": exec_plan.total_steps,
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
) -> HashMap<String, serde_json::Value> {
    let mut all_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    for phase in &plan.phases {
        // Build provided_inputs for this phase: any step input that references
        // a node_id already in all_outputs needs to be embedded.
        let provided = build_provided_inputs(phase, &all_outputs);

        // Spawn workers, collect their stdout.
        let phase_outputs = execute_phase(phase, &provided, python);

        // Merge into global outputs.
        for (node_id, value) in phase_outputs {
            all_outputs.insert(node_id, value);
        }
    }

    all_outputs
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
            serde_json::json!({
                "node_id": s.node_id,
                "function_name": s.function_name,
                "source_file": s.source_file,
                "inputs": s.inputs,
            })
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
    let dag = build_dag(file_args);
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

fn build_dag(file_args: &[String]) -> Dag {
    let paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();
    let mut all_nodes = Vec::new();
    for path in &paths {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Error reading {}: {e}", path.display()));
        let file_str = path.to_string_lossy().to_string();
        let nodes = extract_nodes(&source, &file_str);
        all_nodes.extend(nodes);
    }
    Dag::build(&all_nodes).unwrap_or_else(|e| {
        eprintln!("DAG error: {e}");
        std::process::exit(1);
    })
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
