//! Barca CLI — the main entry point.

use barca_core::dag::Dag;
use barca_core::parse::extract_nodes;
use std::env;
use std::fs;
use std::io::Write;
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
            // Treat as file path (implicit run)
            run_command(&args[1..]);
        }
    }
}

fn run_command(file_args: &[String]) {
    let t0 = Instant::now();

    let (plan_json, node_count, edge_count) = build_plan(file_args);
    let plan_time = t0.elapsed();

    // Initialize DB and persist the plan.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let db_path = ensure_db_dir();
    rt.block_on(async {
        let db = Builder::new_local(&db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        init_schema(&conn).await;
        save_plan(&conn, &plan_json).await;
    });

    // Write plan to temp file for the runner.
    let mut plan_file = tempfile::NamedTempFile::new().expect("failed to create temp file");
    plan_file
        .write_all(plan_json.as_bytes())
        .expect("failed to write plan");
    let plan_path = plan_file.path().to_path_buf();

    // Execute the runner using the sibling Python in the same venv.
    let self_exe = env::current_exe().expect("cannot determine own path");
    let bin_dir = self_exe.parent().expect("binary has no parent dir");
    let python = bin_dir.join("python");

    let t_exec = Instant::now();
    let status = Command::new(&python)
        .args(["-m", "barca._runner"])
        .arg(&plan_path)
        .arg(&db_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", python.display()));

    let exec_time = t_exec.elapsed();
    let total_time = t0.elapsed();

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    eprintln!(
        "[barca] {} nodes, {} edges | plan: {:?} | exec: {:?} | total: {:?}",
        node_count, edge_count, plan_time, exec_time, total_time
    );
}

fn plan_command(file_args: &[String]) {
    let (plan_json, _, _) = build_plan(file_args);
    println!("{plan_json}");
}

/// Ensure .barca/ directory exists and return path to the DB file.
fn ensure_db_dir() -> String {
    let db_dir = PathBuf::from(".barca");
    fs::create_dir_all(&db_dir).ok();
    db_dir.join("metadata.db").to_string_lossy().to_string()
}

/// Initialize the DB schema.
async fn init_schema(conn: &turso::Connection) {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS indexed_nodes (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            function_name TEXT NOT NULL,
            source_file TEXT NOT NULL,
            definition_hash TEXT,
            metadata_json TEXT,
            indexed_at TEXT DEFAULT (datetime('now'))
        )",
        (),
    )
    .await
    .unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS execution_plans (
            plan_id INTEGER PRIMARY KEY AUTOINCREMENT,
            plan_json TEXT NOT NULL,
            created_at TEXT DEFAULT (datetime('now'))
        )",
        (),
    )
    .await
    .unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS materializations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id TEXT NOT NULL,
            plan_id INTEGER,
            run_hash TEXT,
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
}

/// Persist the execution plan to DB.
async fn save_plan(conn: &turso::Connection, plan_json: &str) {
    conn.execute(
        "INSERT INTO execution_plans (plan_json) VALUES (?1)",
        [plan_json],
    )
    .await
    .unwrap();
}

fn build_plan(file_args: &[String]) -> (String, usize, usize) {
    let paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();

    let mut all_nodes = Vec::new();
    for path in &paths {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Error reading {}: {e}", path.display()));
        let file_str = path.to_string_lossy().to_string();
        let nodes = extract_nodes(&source, &file_str);
        all_nodes.extend(nodes);
    }

    let dag = Dag::build(&all_nodes).unwrap_or_else(|e| {
        eprintln!("DAG error: {e}");
        std::process::exit(1);
    });

    let node_count = dag.node_count();
    let edge_count = dag.edge_count();
    let tiers = dag.compute_tiers();
    let total_tiers = tiers.values().copied().max().map(|t| t + 1).unwrap_or(0);

    // Build execution plan JSON.
    let sorted = dag.topo_order();
    let mut steps = Vec::new();
    for node_id in sorted {
        let node = dag.get_node(node_id).unwrap();
        let mut inputs = serde_json::Map::new();
        for (param, upstream_id) in &node.inputs {
            inputs.insert(
                param.clone(),
                serde_json::Value::String(upstream_id.clone()),
            );
        }
        for (param, upstream_id) in &node.collected_inputs {
            inputs.insert(
                param.clone(),
                serde_json::Value::String(upstream_id.clone()),
            );
        }

        steps.push(serde_json::json!({
            "node_id": node_id,
            "kind": node.kind,
            "function_name": node.function_name,
            "source_file": node.source_file,
            "inputs": inputs,
            "partition_keys": node.partition_keys,
            "tier": tiers.get(node_id).copied().unwrap_or(0),
        }));
    }

    let plan = serde_json::json!({
        "plan": {
            "steps": steps,
            "total_tiers": total_tiers,
        }
    });

    (
        serde_json::to_string(&plan).unwrap(),
        node_count,
        edge_count,
    )
}
