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
        "get" => get_command(&args[2..]),
        "plan" => plan_command(&args[2..]),
        "--help" | "-h" => {
            println!("barca — invisible asset orchestrator");
            println!();
            println!("Commands:");
            println!("  get <target> <file>     Get a fresh asset (cache-aware)");
            println!("  run <file> [file ...]   Parse, plan, and execute all assets");
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

/// `barca get <target> <file>` — get a fresh asset, cache-aware.
fn get_command(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: barca get <target_function> <file>");
        std::process::exit(1);
    }

    let target_name = &args[0];
    let file_args = &args[1..];
    let t0 = Instant::now();
    let python = find_python();

    let dag = build_dag(file_args, &python);

    // Find the target node by function name.
    let target_id = dag
        .topo_order()
        .into_iter()
        .find(|id| id.ends_with(&format!(":{target_name}")))
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            eprintln!("Asset '{}' not found", target_name);
            std::process::exit(1);
        });

    // Resolve subgraph upstream of target.
    let subgraph_ids = dag.subgraph(&target_id);

    // Initialize DB + check cache.
    let db_path = ensure_db_dir();
    init_db_sync(&db_path);

    // For each node in the subgraph (topo order), compute run_hash and check cache.
    let mut cached_outputs: HashMap<String, serde_json::Value> = HashMap::new();
    let mut cached_run_hashes: HashMap<String, String> = HashMap::new();
    let mut stale_ids: Vec<String> = Vec::new();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for node_id in &subgraph_ids {
        let node = dag.get_node(node_id).unwrap();

        // Compute run_hash: definition_hash + upstream run_hashes.
        let upstream_hashes: Vec<&str> = dag
            .upstream(node_id)
            .iter()
            .filter_map(|uid| cached_run_hashes.get(*uid).map(|s| s.as_str()))
            .collect();
        let run_h = barca_core::hash::run_hash(&node.definition_hash, None, &upstream_hashes, None);

        // Check DB for cached output with this run_hash.
        let cached = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query(
                    "SELECT output_json FROM materializations WHERE node_id = ?1 AND run_hash = ?2 ORDER BY id DESC LIMIT 1",
                    [node_id.to_string(), run_h.clone()],
                )
                .await
                .unwrap();
            if let Some(row) = rows.next().await.unwrap() {
                let output_json: String = row.get::<String>(0).unwrap();
                Some(output_json)
            } else {
                None
            }
        });

        if let Some(output_json) = cached {
            // Cache hit — use stored output.
            let value: serde_json::Value = serde_json::from_str(&output_json).unwrap_or_default();
            cached_outputs.insert(node_id.to_string(), value);
            cached_run_hashes.insert(node_id.to_string(), run_h);
        } else {
            // Stale — needs execution. All downstream is also stale.
            stale_ids.push(node_id.to_string());
            // Mark all remaining nodes as stale (they depend on this)
            break;
        }
    }

    // If target is cached and not stale, return immediately.
    if stale_ids.is_empty()
        && let Some(output) = cached_outputs.get(&target_id)
    {
        println!(
            "{}",
            serde_json::json!({
                "elapsed_seconds": t0.elapsed().as_secs_f64(),
                "steps_executed": 0,
                "cached": true,
                "final_output": output,
            })
        );
        return;
    }

    // Plan from the full DAG, then filter to only the target's subgraph.
    let pool_size = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let config = ResourceConfig {
        pool_size,
        concurrency_groups: HashMap::new(),
    };
    let full_plan = planner::plan_from_dag(&dag, &config);

    // Filter plan to only include steps in the target's subgraph.
    let subgraph_set: std::collections::HashSet<&str> = subgraph_ids.iter().copied().collect();
    let exec_plan = ExecutionPlan {
        phases: full_plan
            .phases
            .into_iter()
            .map(|phase| {
                let filtered_streams: Vec<barca_core::planner::WorkerStream> = phase
                    .streams
                    .into_iter()
                    .map(|s| barca_core::planner::WorkerStream {
                        stream_id: s.stream_id,
                        steps: s
                            .steps
                            .into_iter()
                            .filter(|st| {
                                // Include if the base node_id (without partition suffix) is in subgraph.
                                let base_id = st.node_id.split('[').next().unwrap_or(&st.node_id);
                                subgraph_set.contains(base_id)
                            })
                            .collect(),
                    })
                    .filter(|s| !s.steps.is_empty())
                    .collect();
                barca_core::planner::Phase {
                    reason: phase.reason,
                    streams: filtered_streams,
                }
            })
            .filter(|p| !p.streams.is_empty())
            .collect(),
        total_steps: 0, // recalculated below
    };
    let exec_plan = ExecutionPlan {
        total_steps: exec_plan
            .phases
            .iter()
            .flat_map(|p| &p.streams)
            .map(|s| s.steps.len())
            .sum(),
        ..exec_plan
    };

    // Dispatch, seeding with cached outputs. Skip steps that are already cached.
    let mut all_outputs = cached_outputs.clone();
    let mut steps_executed = 0;

    for phase in &exec_plan.phases {
        let expanded_phase = expand_pending_partitions(phase, &all_outputs, pool_size);
        let phase_ref = expanded_phase.as_ref().unwrap_or(phase);

        // Filter out cached steps from this phase.
        let filtered_streams: Vec<barca_core::planner::WorkerStream> = phase_ref
            .streams
            .iter()
            .map(|s| {
                let uncached_steps: Vec<barca_core::planner::StreamStep> = s
                    .steps
                    .iter()
                    .filter(|st| !all_outputs.contains_key(&st.node_id))
                    .cloned()
                    .collect();
                barca_core::planner::WorkerStream {
                    stream_id: s.stream_id.clone(),
                    steps: uncached_steps,
                }
            })
            .filter(|s| !s.steps.is_empty())
            .collect();

        if filtered_streams.is_empty() {
            continue; // All steps cached — skip phase entirely.
        }

        let filtered_phase = barca_core::planner::Phase {
            reason: phase_ref.reason.clone(),
            streams: filtered_streams,
        };

        steps_executed += filtered_phase
            .streams
            .iter()
            .map(|s| s.steps.len())
            .sum::<usize>();

        let provided = build_provided_inputs(&filtered_phase, &all_outputs);
        let phase_outputs = execute_phase(&filtered_phase, &provided, &python);
        for (node_id, value) in phase_outputs {
            all_outputs.insert(node_id, value);
        }
    }

    // Compute run_hashes for ALL nodes (cached + newly executed) in topo order,
    // then persist newly executed ones.
    let mut final_run_hashes: HashMap<String, String> = HashMap::new();
    for node_id in &subgraph_ids {
        let Some(node) = dag.get_node(node_id) else {
            continue;
        };
        let upstream_hashes: Vec<&str> = dag
            .upstream(node_id)
            .iter()
            .filter_map(|uid| final_run_hashes.get(*uid).map(|s| s.as_str()))
            .collect();
        let run_h = barca_core::hash::run_hash(&node.definition_hash, None, &upstream_hashes, None);
        final_run_hashes.insert(node_id.to_string(), run_h);
    }

    // Persist only newly executed nodes (not ones that were already cached).
    rt.block_on(async {
        let db = Builder::new_local(&db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        for node_id in &subgraph_ids {
            if cached_outputs.contains_key(*node_id) {
                continue;
            }
            let Some(output) = all_outputs.get(*node_id) else {
                continue;
            };
            let Some(run_h) = final_run_hashes.get(*node_id) else {
                continue;
            };
            let output_json = serde_json::to_string(output).unwrap_or_default();
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, output_json) VALUES (?1, ?2, ?3)",
                [node_id.to_string(), run_h.clone(), output_json],
            )
            .await
            .ok();
        }
    });

    let final_output = all_outputs.get(&target_id);

    println!(
        "{}",
        serde_json::json!({
            "elapsed_seconds": t0.elapsed().as_secs_f64(),
            "steps_executed": steps_executed,
            "cached": false,
            "final_output": final_output,
        })
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

    // Group expanded steps by partition key for aligned execution.
    let mut by_partition: HashMap<String, Vec<barca_core::planner::StreamStep>> = HashMap::new();
    for step in all_expanded_steps {
        let key = step
            .partition
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        by_partition.entry(key).or_default().push(step);
    }

    // Build work units: each partition group + passthrough steps.
    let mut work_units: Vec<Vec<barca_core::planner::StreamStep>> = Vec::new();
    if !passthrough_steps.is_empty() {
        work_units.push(passthrough_steps);
    }
    let mut keys: Vec<String> = by_partition.keys().cloned().collect();
    keys.sort();
    for key in keys {
        work_units.push(by_partition.remove(&key).unwrap());
    }

    // Distribute work units across streams via bin-packing.
    let num_streams = work_units.len().min(pool_size).max(1);
    let mut streams: Vec<Vec<barca_core::planner::StreamStep>> = vec![Vec::new(); num_streams];
    let mut sizes: Vec<usize> = vec![0; num_streams];

    for unit in work_units {
        let target = sizes
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| *s)
            .map(|(i, _)| i)
            .unwrap_or(0);
        sizes[target] += unit.len();
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
/// For cross-phase deps, includes the output value so workers can resolve inputs.
/// For partition-aligned deps, `serialize_batch` rewrites input IDs to include
/// partition suffixes, so the worker finds them in its local cache.
fn build_provided_inputs(
    phase: &Phase,
    all_outputs: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut provided: HashMap<String, serde_json::Value> = HashMap::new();

    for stream in &phase.streams {
        for step in &stream.steps {
            // Collect all upstream IDs this step needs (with partition suffix if applicable).
            let suffix = if step.partition.is_empty() {
                String::new()
            } else {
                format!(
                    "[{}]",
                    step.partition
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };

            for upstream_id in step.inputs.values() {
                // Try direct match first.
                if let Some(output) = all_outputs.get(upstream_id) {
                    provided
                        .entry(upstream_id.clone())
                        .or_insert_with(|| output.clone());
                    continue;
                }
                // Try partition-aligned match.
                if !suffix.is_empty() {
                    let aligned_id = format!("{upstream_id}{suffix}");
                    if let Some(output) = all_outputs.get(&aligned_id) {
                        provided.entry(aligned_id).or_insert_with(|| output.clone());
                    }
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
            // For partitioned steps, rewrite input references to include the
            // partition suffix so the worker's cache can find them.
            let inputs = if s.partition.is_empty() {
                s.inputs.clone()
            } else {
                let suffix = s
                    .partition
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                s.inputs
                    .iter()
                    .map(|(param, upstream_id)| {
                        let aligned_id = format!("{upstream_id}[{suffix}]");
                        (param.clone(), aligned_id)
                    })
                    .collect()
            };

            let mut step = serde_json::json!({
                "node_id": s.node_id,
                "function_name": s.function_name,
                "source_file": s.source_file,
                "inputs": inputs,
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
