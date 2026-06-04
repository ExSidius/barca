//! CLI commands — run, get, plan, and shared setup.

use barca_core::dag::Dag;
use barca_core::parse::extract_nodes;
use barca_core::planner::{self, ExecutionPlan, Phase, ResourceConfig};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use turso::Builder;

use crate::cache;
use crate::db;
use crate::dispatch;

// ─── Shared setup ────────────────────────────────────────────────────────────

pub fn find_python() -> PathBuf {
    let self_exe = env::current_exe().expect("cannot determine own path");
    let bin_dir = self_exe.parent().expect("binary has no parent dir");
    bin_dir.join("python")
}

fn default_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

// ─── run ─────────────────────────────────────────────────────────────────────

pub fn run_command(file_args: &[String]) {
    let t0 = Instant::now();
    let python = find_python();

    let dag = build_dag(file_args, &python);
    let node_count = dag.node_count();
    let edge_count = dag.edge_count();

    let pool_size = default_pool_size();
    let config = ResourceConfig {
        pool_size,
        concurrency_groups: HashMap::new(),
    };
    let exec_plan = planner::plan_from_dag(&dag, &config);
    let plan_time = t0.elapsed();

    let db_path = db::ensure_db_dir();
    db::init_db_sync(&db_path);

    let t_exec = Instant::now();
    let all_outputs = dispatch::dispatch_plan(&exec_plan, &python, &db_path, pool_size);
    let exec_time = t_exec.elapsed();

    // Compute run hashes for all outputs so `barca get` can reuse them.
    let mut run_hashes: HashMap<String, String> = HashMap::new();
    // Process outputs in topo order so upstream hashes are available.
    let topo: Vec<String> = dag
        .topo_order()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    for base_id in &topo {
        if let Some(node) = dag.get_node(base_id) {
            // Collect all output keys for this node (unpartitioned + partitioned).
            let display_ids: Vec<String> = all_outputs
                .keys()
                .filter(|k| *k == base_id || k.starts_with(&format!("{base_id}[")))
                .cloned()
                .collect();
            for display_id in display_ids {
                let sid = barca_core::StepId::parse(&display_id);
                let partition_key = if sid.partition.is_empty() {
                    None
                } else {
                    Some(sid.partition.suffix())
                };
                let upstream_ids: Vec<String> = node
                    .resolved_inputs
                    .values()
                    .chain(node.resolved_collected.values())
                    .cloned()
                    .collect();
                let run_h = cache::compute_run_hash(
                    &node.definition_hash,
                    partition_key.as_deref(),
                    upstream_ids.iter(),
                    &run_hashes,
                );
                run_hashes.insert(display_id, run_h);
            }
        }
    }

    db::persist_outputs_sync(&db_path, &all_outputs, &run_hashes);

    let total_executed = all_outputs.len();
    let last_planned_id = exec_plan
        .phases
        .last()
        .and_then(|p| p.streams.last())
        .and_then(|s| s.steps.last())
        .map(|s| s.step_id.display())
        .unwrap_or_default();
    let last_output = all_outputs.get(&last_planned_id).or_else(|| {
        all_outputs
            .iter()
            .filter(|(k, _)| k.starts_with(&last_planned_id))
            .map(|(_, v)| v)
            .last()
    });
    if let Some(oref) = last_output {
        println!(
            "{}",
            serde_json::json!({
                "elapsed_seconds": exec_time.as_secs_f64(),
                "steps_executed": total_executed,
                "phases": exec_plan.phases.len(),
                "final_output": {
                    "artifact_path": oref.path,
                    "artifact_format": oref.format,
                    "artifact_size_bytes": oref.size_bytes,
                },
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

// ─── get ─────────────────────────────────────────────────────────────────────

pub fn get_command(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: barca get <target_function> <file>");
        std::process::exit(1);
    }

    let target_name = &args[0];
    let file_args = &args[1..];
    let t0 = Instant::now();
    let python = find_python();

    let dag = build_dag(file_args, &python);

    let target_id = dag
        .topo_order()
        .into_iter()
        .find(|id| {
            id.ends_with(&format!(":{target_name}"))
                || *id == target_name.as_str()
                || id.ends_with(target_name.as_str())
        })
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            eprintln!("Asset '{}' not found", target_name);
            std::process::exit(1);
        });

    let subgraph_ids = dag.subgraph(&target_id);

    let db_path = db::ensure_db_dir();
    db::init_db_sync(&db_path);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let pool_size = default_pool_size();
    let config = ResourceConfig {
        pool_size,
        concurrency_groups: HashMap::new(),
    };
    let full_plan = planner::plan_from_dag(&dag, &config);
    let exec_plan = filter_plan_to_subgraph(full_plan, &subgraph_ids);

    // Open DB connection once for all cache checks + persist.
    let db = rt.block_on(async { Builder::new_local(&db_path).build().await.unwrap() });
    let conn = db.connect().unwrap();

    let mut cached_node_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cached_run_hashes: HashMap<String, String> = HashMap::new();
    let mut all_outputs: HashMap<String, dispatch::OutputRef> = HashMap::new();
    let mut steps_executed = 0;

    for phase in &exec_plan.phases {
        let expanded_phase = dispatch::expand_pending_partitions(phase, &all_outputs, pool_size);
        let phase_ref = expanded_phase.as_ref().unwrap_or(phase);

        let mut uncached_streams: Vec<barca_core::planner::WorkerStream> = Vec::new();

        for stream in &phase_ref.streams {
            let mut uncached_steps: Vec<barca_core::planner::StreamStep> = Vec::new();

            for step in &stream.steps {
                let base_id = step.step_id.base_id();
                let display_id = step.step_id.display();
                let base_node = dag.get_node(base_id);

                // Sensors always re-run.
                if base_node.is_some_and(|n| n.kind() == barca_core::NodeKind::Sensor) {
                    uncached_steps.push(step.clone());
                    continue;
                }

                let def_hash = base_node.map(|n| n.definition_hash.as_str()).unwrap_or("");
                let partition_key = if step.step_id.partition.is_empty() {
                    None
                } else {
                    Some(step.step_id.partition.suffix())
                };

                let run_h = cache::compute_run_hash(
                    def_hash,
                    partition_key.as_deref(),
                    step.inputs.values(),
                    &cached_run_hashes,
                );

                // Check DB for cached artifact (reusing single connection).
                let cached = rt.block_on(async {
                    let mut rows = conn
                        .query(
                            "SELECT artifact_path, artifact_format, artifact_size_bytes FROM materializations WHERE node_id = ?1 AND run_hash = ?2 ORDER BY id DESC LIMIT 1",
                            [display_id.clone(), run_h.clone()],
                        )
                        .await
                        .unwrap();
                    rows.next().await.unwrap().and_then(|row| {
                        Some(dispatch::OutputRef {
                            path: row.get::<String>(0).ok()?,
                            format: row.get::<String>(1).ok()?,
                            size_bytes: row.get::<i64>(2).ok()? as u64,
                        })
                    })
                });

                if let Some(oref) = cached {
                    all_outputs.insert(display_id.clone(), oref);
                    cached_node_ids.insert(display_id.clone());
                    cached_run_hashes.insert(display_id, run_h);
                } else {
                    uncached_steps.push(step.clone());
                }
            }

            if !uncached_steps.is_empty() {
                uncached_streams.push(barca_core::planner::WorkerStream {
                    stream_id: stream.stream_id.clone(),
                    steps: uncached_steps,
                });
            }
        }

        if uncached_streams.is_empty() {
            continue;
        }

        let filtered_phase = Phase {
            reason: phase_ref.reason.clone(),
            streams: uncached_streams,
        };

        steps_executed += filtered_phase
            .streams
            .iter()
            .map(|s| s.steps.len())
            .sum::<usize>();

        let provided = dispatch::build_provided_inputs(&filtered_phase, &all_outputs);
        let phase_outputs = dispatch::execute_phase(&filtered_phase, &provided, &python);

        // Process in stream-step order (topo order) so upstream hashes are computed first.
        let step_order: Vec<String> = filtered_phase
            .streams
            .iter()
            .flat_map(|s| s.steps.iter().map(|st| st.step_id.display()))
            .collect();
        for node_id in step_order {
            let Some(oref) = phase_outputs.get(&node_id).cloned() else {
                continue;
            };
            let sid = barca_core::StepId::parse(&node_id);
            let def_hash = dag
                .get_node(sid.base_id())
                .map(|n| n.definition_hash.as_str())
                .unwrap_or("");
            let partition_key = if sid.partition.is_empty() {
                None
            } else {
                Some(sid.partition.suffix())
            };

            let upstream_ids: Vec<String> = dag
                .get_node(sid.base_id())
                .map(|n| {
                    n.resolved_inputs
                        .values()
                        .chain(n.resolved_collected.values())
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let run_h = cache::compute_run_hash(
                def_hash,
                partition_key.as_deref(),
                upstream_ids.iter(),
                &cached_run_hashes,
            );
            cached_run_hashes.insert(node_id.clone(), run_h);
            all_outputs.insert(node_id, oref);
        }
    }

    // Persist newly executed outputs (reusing single connection).
    rt.block_on(async {
        for (node_id, oref) in &all_outputs {
            if cached_node_ids.contains(node_id) {
                continue;
            }
            let Some(run_h) = cached_run_hashes.get(node_id) else {
                continue;
            };
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, artifact_path, artifact_format, artifact_size_bytes) VALUES (?1, ?2, ?3, ?4, ?5)",
                [
                    node_id.clone(),
                    run_h.clone(),
                    oref.path.clone(),
                    oref.format.clone(),
                    oref.size_bytes.to_string(),
                ],
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
            "final_output": final_output.map(|oref| serde_json::json!({
                "artifact_path": oref.path,
                "artifact_format": oref.format,
                "artifact_size_bytes": oref.size_bytes,
            })),
        })
    );
}

/// Filter an execution plan to only include steps in the target's subgraph.
fn filter_plan_to_subgraph(plan: ExecutionPlan, subgraph_ids: &[&str]) -> ExecutionPlan {
    let subgraph_set: std::collections::HashSet<&str> = subgraph_ids.iter().copied().collect();
    let exec_plan = ExecutionPlan {
        phases: plan
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
                                let base_id = st.step_id.base_id();
                                subgraph_set.contains(base_id)
                            })
                            .collect(),
                    })
                    .filter(|s| !s.steps.is_empty())
                    .collect();
                Phase {
                    reason: phase.reason,
                    streams: filtered_streams,
                }
            })
            .filter(|p| !p.streams.is_empty())
            .collect(),
        total_steps: 0,
    };
    ExecutionPlan {
        total_steps: exec_plan
            .phases
            .iter()
            .flat_map(|p| &p.streams)
            .map(|s| s.steps.len())
            .sum(),
        ..exec_plan
    }
}

// ─── plan ────────────────────────────────────────────────────────────────────

pub fn plan_command(file_args: &[String]) {
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
                        "steps": s.steps.iter().map(|st| st.step_id.display()).collect::<Vec<_>>(),
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

// ─── DAG construction ────────────────────────────────────────────────────────

pub fn build_dag(file_args: &[String], python: &PathBuf) -> Dag {
    let paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();
    let mut all_nodes = Vec::new();
    let mut file_sources: HashMap<String, String> = HashMap::new();

    for path in &paths {
        let source = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Error reading {}: {e}", path.display()));
        let file_str = path.to_string_lossy().to_string();
        let nodes = extract_nodes(&source, &file_str).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        file_sources.insert(stem, source.clone());
        if let Some(parent) = path.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let ep = entry.path();
                    if ep.extension().map(|e| e == "py").unwrap_or(false) && ep != *path {
                        let estem = ep
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        if let std::collections::hash_map::Entry::Vacant(e) =
                            file_sources.entry(estem)
                            && let Ok(content) = fs::read_to_string(&ep)
                        {
                            e.insert(content);
                        }
                    }
                }
            }
        }
        all_nodes.extend(nodes);
    }

    // Recompute cone_hashes with cross-file import resolution.
    for node in &mut all_nodes {
        let stem_from_path = node
            .source_file
            .rsplit('/')
            .next()
            .unwrap_or("")
            .replace(".py", "");
        let source = file_sources.get(&stem_from_path).or_else(|| {
            let stem = PathBuf::from(&node.source_file);
            let s = stem.file_stem()?.to_str()?;
            file_sources.get(s)
        });
        if let Some(src) = source {
            node.cone_hash =
                barca_core::cone::cone_hash_with_imports(src, &node.function_name, &file_sources);
        }
    }

    resolve_dynamic_partitions(&mut all_nodes, python);

    Dag::build(&all_nodes).unwrap_or_else(|e| {
        eprintln!("DAG error: {e}");
        std::process::exit(1);
    })
}

/// Resolve PartitionSpec::Dynamic by evaluating Python expressions.
fn resolve_dynamic_partitions(nodes: &mut [barca_core::model::ExtractedNode], python: &PathBuf) {
    for node in nodes.iter_mut() {
        let mut resolved: Vec<(String, Vec<barca_core::model::PartitionValue>)> = Vec::new();

        for (dim, spec) in &node.partitions {
            if let barca_core::model::PartitionSpec::Dynamic { source_text } = spec {
                let module_path = std::path::Path::new(&node.source_file)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&node.source_file));
                // Write the evaluation script to a temp file to avoid shell escaping
                // issues with paths or expressions containing quotes/backslashes.
                let script = format!(
                    "import json, importlib.util, sys\n\
                     _spec = importlib.util.spec_from_file_location('_m', sys.argv[1])\n\
                     _mod = importlib.util.module_from_spec(_spec)\n\
                     _spec.loader.exec_module(_mod)\n\
                     _ns = vars(_mod); _ns['__builtins__'] = __builtins__\n\
                     print(json.dumps(eval(sys.argv[2], _ns)))\n"
                );
                let mut script_file =
                    tempfile::NamedTempFile::new().expect("failed to create temp file");
                use std::io::Write;
                script_file
                    .write_all(script.as_bytes())
                    .expect("failed to write script");
                let script_path = script_file.path().to_path_buf();
                let output = Command::new(python)
                    .arg(&script_path)
                    .arg(module_path.to_string_lossy().as_ref())
                    .arg(source_text)
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

        for (dim, values) in resolved {
            node.partitions
                .insert(dim, barca_core::model::PartitionSpec::Static { values });
        }
    }
}
