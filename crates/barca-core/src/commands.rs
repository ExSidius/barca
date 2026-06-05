//! Engine commands — get, plan, history, stats. Return typed results; callers handle display.

use crate::BarcaError;
use crate::cache;
use crate::dag::Dag;
use crate::db;
use crate::dispatch;
use crate::dispatch::OutputRef;
use crate::parse::extract_nodes;
use crate::planner::{self, ExecutionPlan, Phase, ResourceConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use turso::Builder;

// ─── Result types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetResult {
    pub run_id: String,
    pub elapsed_seconds: f64,
    pub steps_executed: usize,
    pub phases: usize,
    pub final_output: Option<OutputRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResult {
    pub total_steps: usize,
    pub phases: Vec<PlanPhase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPhase {
    pub reason: String,
    pub streams: Vec<PlanStream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStream {
    pub stream_id: String,
    pub steps: Vec<String>,
}

// ─── Shared setup ────────────────────────────────────────────────────────────

pub fn find_python() -> PathBuf {
    // Look for sibling python in the same bin/ directory as the barca binary.
    if let Ok(self_exe) = env::current_exe() {
        if let Some(bin_dir) = self_exe.parent() {
            let candidate = bin_dir.join("python");
            if candidate.exists() {
                return candidate;
            }
            let candidate3 = bin_dir.join("python3");
            if candidate3.exists() {
                return candidate3;
            }
        }
    }
    // Fall back to PATH.
    PathBuf::from("python3")
}

fn default_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

// ─── get ─────────────────────────────────────────────────────────────────────

pub fn get(
    target_name: Option<&str>,
    file_args: &[String],
    python: &PathBuf,
    no_cache: bool,
    agent_mode: bool,
) -> Result<GetResult, BarcaError> {
    let t0 = Instant::now();
    let run_id = db::generate_run_id();

    let dag = build_dag(file_args, python)?;

    // Resolve target: if Some, find it and extract subgraph; if None, use full DAG.
    let target_id: Option<String> = match target_name {
        Some(name) => {
            let id = dag
                .topo_order()
                .into_iter()
                .find(|id| id.ends_with(&format!(":{name}")) || *id == name || id.ends_with(name))
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    let available: Vec<&str> = dag.topo_order();
                    BarcaError::AssetNotFound(name.to_string(), available.join(", "))
                })?;
            Some(id)
        }
        None => None,
    };

    let pool_size = default_pool_size();
    let config = ResourceConfig {
        pool_size,
        concurrency_groups: HashMap::new(),
    };
    let full_plan = planner::plan_from_dag(&dag, &config);
    let exec_plan = if let Some(ref tid) = target_id {
        let subgraph_ids = dag.subgraph(tid);
        filter_plan_to_subgraph(full_plan, &subgraph_ids)
    } else {
        full_plan
    };

    let db_path = db::ensure_db_dir();
    db::init_db_sync(&db_path);

    db::create_run_sync(
        &db_path,
        &run_id,
        "get",
        &file_args.join(" "),
        target_name,
        Some(exec_plan.total_steps),
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let db = rt.block_on(async { Builder::new_local(&db_path).build().await.unwrap() });
    let conn = db.connect().unwrap();

    let mut cached_node_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cached_run_hashes: HashMap<String, String> = HashMap::new();
    let mut phase_error: Option<String> = None;
    let mut all_outputs: HashMap<String, dispatch::OutputRef> = HashMap::new();
    let mut steps_executed = 0;

    // Progress bar setup.
    let total_steps = exec_plan.total_steps;
    let all_node_ids: Vec<String> = exec_plan
        .phases
        .iter()
        .flat_map(|p| &p.streams)
        .flat_map(|s| &s.steps)
        .map(|st| st.step_id.display())
        .collect();
    let avg_times = db::get_avg_elapsed_sync(&db_path, &all_node_ids);
    let total_estimated: f64 = all_node_ids
        .iter()
        .filter_map(|nid| avg_times.get(nid))
        .sum();
    let mut elapsed_so_far: f64 = 0.0;
    let mut completed_steps: usize = 0;

    // Create indicatif progress bar for human mode, plain text for agent mode.
    let pb = if !agent_mode && total_steps > 0 {
        use indicatif::{ProgressBar, ProgressStyle};
        let bar = ProgressBar::new(total_steps as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "[barca] {prefix} {bar:20.cyan/dim} {pos}/{len} | {wide_msg}",
            )
            .unwrap()
            .progress_chars("█▓░"),
        );
        if total_estimated > 0.0 {
            bar.set_prefix(format!("~{:.0}s left", total_estimated));
        } else {
            bar.set_prefix("starting");
        }
        bar.set_message("");
        Some(bar)
    } else {
        None
    };

    for phase in &exec_plan.phases {
        let expanded_phase = dispatch::expand_pending_partitions(phase, &all_outputs, pool_size);
        let phase_ref = expanded_phase.as_ref().unwrap_or(phase);

        let mut uncached_streams: Vec<crate::planner::WorkerStream> = Vec::new();

        for stream in &phase_ref.streams {
            let mut uncached_steps: Vec<crate::planner::StreamStep> = Vec::new();

            for step in &stream.steps {
                let base_id = step.step_id.base_id();
                let display_id = step.step_id.display();
                let base_node = dag.get_node(base_id);

                // Sensors always re-run.
                if base_node.is_some_and(|n| n.kind() == crate::NodeKind::Sensor) {
                    uncached_steps.push(step.clone());
                    continue;
                }

                // Skip cache lookups when no_cache is set.
                if no_cache {
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
                            elapsed_seconds: None,
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
                uncached_streams.push(crate::planner::WorkerStream {
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

        // Progress callback — update bar as each step completes from any worker.
        let mut on_step = |node_id: &str, output: &dispatch::OutputRef| {
            if let Some(e) = output.elapsed_seconds {
                elapsed_so_far += e;
            }
            completed_steps += 1;
            if let Some(ref bar) = pb {
                bar.set_position(completed_steps as u64);
                let remaining = if total_estimated > 0.0 {
                    (total_estimated - elapsed_so_far).max(0.0)
                } else if completed_steps > 0 {
                    let avg = elapsed_so_far / completed_steps as f64;
                    avg * (total_steps - completed_steps) as f64
                } else {
                    0.0
                };
                let short_name = node_id.rsplit(':').next().unwrap_or(node_id);
                if remaining > 0.5 {
                    bar.set_prefix(format!("~{remaining:.0}s left"));
                } else {
                    bar.set_prefix("finishing");
                }
                bar.set_message(format!("{short_name} done"));
            } else if agent_mode {
                eprintln!(
                    "[barca] step:{} completed {:.1}s ({}/{})",
                    node_id,
                    output.elapsed_seconds.unwrap_or(0.0),
                    completed_steps,
                    total_steps
                );
            }
        };

        let phase_result =
            dispatch::execute_phase(&filtered_phase, &provided, python, Some(&mut on_step));
        if phase_error.is_none() {
            phase_error = phase_result.error;
        }
        let phase_outputs = phase_result.outputs;

        let step_order: Vec<String> = filtered_phase
            .streams
            .iter()
            .flat_map(|s| s.steps.iter().map(|st| st.step_id.display()))
            .collect();
        for node_id in step_order {
            let Some(oref) = phase_outputs.get(&node_id).cloned() else {
                continue;
            };
            let sid = crate::StepId::parse(&node_id);
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

        // If this phase had a worker failure, stop after collecting partial results.
        if phase_error.is_some() {
            break;
        }
    }

    // Finish progress bar.
    if let Some(ref bar) = pb {
        if steps_executed > 0 {
            bar.finish_and_clear();
            eprintln!(
                "[barca] {}/{} steps done in {:.1}s",
                completed_steps, total_steps, elapsed_so_far
            );
        } else {
            bar.finish_and_clear();
        }
    } else if agent_mode && steps_executed > 0 {
        eprintln!(
            "[barca] {}/{} steps | done in {:.1}s",
            completed_steps, total_steps, elapsed_so_far
        );
    }

    // Persist all executed outputs (including partial results on failure).
    rt.block_on(async {
        for (node_id, oref) in &all_outputs {
            if cached_node_ids.contains(node_id) {
                continue;
            }
            let Some(run_h) = cached_run_hashes.get(node_id) else {
                continue;
            };
            let elapsed_str = oref
                .elapsed_seconds
                .map(|e| e.to_string())
                .unwrap_or_default();
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, artifact_path, artifact_format, artifact_size_bytes, elapsed_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                [
                    node_id.clone(),
                    run_h.clone(),
                    oref.path.clone(),
                    oref.format.clone(),
                    oref.size_bytes.to_string(),
                    elapsed_str,
                ],
            )
            .await
            .ok();
        }
    });

    let steps_cached = cached_node_ids.len();
    let elapsed = t0.elapsed().as_secs_f64();

    // Propagate worker error after persisting partial results.
    if let Some(error) = phase_error {
        db::finish_run_sync(
            &db_path,
            &run_id,
            "failed",
            steps_executed,
            steps_cached,
            elapsed,
        );
        return Err(BarcaError::WorkerFailed(error));
    }

    db::finish_run_sync(
        &db_path,
        &run_id,
        "success",
        steps_executed,
        steps_cached,
        elapsed,
    );

    // Determine final_output: use target if specified, otherwise last planned step.
    let final_output = if let Some(ref tid) = target_id {
        all_outputs.get(tid).cloned().or_else(|| {
            // For partitioned targets, sort by key for deterministic output.
            let prefix = format!("{tid}[");
            let mut matches: Vec<_> = all_outputs
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix))
                .collect();
            matches.sort_by_key(|(k, _)| k.clone());
            matches.first().map(|(_, v)| (*v).clone())
        })
    } else {
        // No target: return the last planned step's output.
        let last_planned_id = exec_plan
            .phases
            .last()
            .and_then(|p| p.streams.last())
            .and_then(|s| s.steps.last())
            .map(|s| s.step_id.display())
            .unwrap_or_default();
        all_outputs.get(&last_planned_id).cloned().or_else(|| {
            let mut matches: Vec<_> = all_outputs
                .iter()
                .filter(|(k, _)| k.starts_with(&last_planned_id))
                .collect();
            matches.sort_by_key(|(k, _)| k.clone());
            matches.first().map(|(_, v)| (*v).clone())
        })
    };

    Ok(GetResult {
        run_id,
        elapsed_seconds: elapsed,
        steps_executed,
        phases: exec_plan.phases.len(),
        final_output,
    })
}

// ─── plan ────────────────────────────────────────────────────────────────────

pub fn plan(file_args: &[String], python: &PathBuf) -> Result<PlanResult, BarcaError> {
    let dag = build_dag(file_args, python)?;
    let config = ResourceConfig {
        pool_size: 10,
        concurrency_groups: HashMap::new(),
    };
    let plan = planner::plan_from_dag(&dag, &config);

    Ok(PlanResult {
        total_steps: plan.total_steps,
        phases: plan
            .phases
            .iter()
            .map(|p| PlanPhase {
                reason: format!("{:?}", p.reason),
                streams: p
                    .streams
                    .iter()
                    .map(|s| PlanStream {
                        stream_id: s.stream_id.clone(),
                        steps: s.steps.iter().map(|st| st.step_id.display()).collect(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

// ─── history ──────────────────────────────────────────────────────────────────

pub fn history(limit: usize) -> Result<Vec<db::RunRecord>, BarcaError> {
    let db_path = db::ensure_db_dir();
    db::init_db_sync(&db_path);
    Ok(db::get_recent_runs_sync(&db_path, limit))
}

// ─── stats ────────────────────────────────────────────────────────────────────

pub fn stats(
    target_name: &str,
    file_args: &[String],
    python: &PathBuf,
) -> Result<db::AssetStats, BarcaError> {
    let dag = build_dag(file_args, python)?;

    let target_id = dag
        .topo_order()
        .into_iter()
        .find(|id| {
            id.ends_with(&format!(":{target_name}"))
                || *id == target_name
                || id.ends_with(target_name)
        })
        .map(|s| s.to_string())
        .ok_or_else(|| {
            let available: Vec<&str> = dag.topo_order();
            BarcaError::AssetNotFound(target_name.to_string(), available.join(", "))
        })?;

    let db_path = db::ensure_db_dir();
    db::init_db_sync(&db_path);
    Ok(db::get_asset_stats_sync(&db_path, &target_id))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn filter_plan_to_subgraph(plan: ExecutionPlan, subgraph_ids: &[&str]) -> ExecutionPlan {
    let subgraph_set: std::collections::HashSet<&str> = subgraph_ids.iter().copied().collect();
    let exec_plan = ExecutionPlan {
        phases: plan
            .phases
            .into_iter()
            .map(|phase| {
                let filtered_streams: Vec<crate::planner::WorkerStream> = phase
                    .streams
                    .into_iter()
                    .map(|s| crate::planner::WorkerStream {
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

// ─── DAG construction ────────────────────────────────────────────────────────

pub fn build_dag(file_args: &[String], python: &PathBuf) -> Result<Dag, BarcaError> {
    let paths: Vec<PathBuf> = file_args.iter().map(PathBuf::from).collect();
    let mut all_nodes = Vec::new();
    let mut file_sources: HashMap<String, String> = HashMap::new();

    for path in &paths {
        let source = fs::read_to_string(path)
            .map_err(|e| BarcaError::Other(format!("{}: {e}", path.display())))?;
        let file_str = path.to_string_lossy().to_string();
        let nodes =
            extract_nodes(&source, &file_str).map_err(|e| BarcaError::Parse(e.to_string()))?;
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        file_sources.insert(stem, source.clone());
        if let Some(parent) = path.parent() {
            // Scan subdirectories FIRST — packages (__init__.py) take precedence
            // over same-named sibling .py files, matching Python's import semantics.
            scan_subdirectories(parent, parent, &mut file_sources);
            // Then scan sibling .py files (flat) — or_insert_with is a no-op if
            // a package with the same name was already registered above.
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
                crate::cone::cone_hash_with_imports(src, &node.function_name, &file_sources);
        }
    }

    resolve_dynamic_partitions(&mut all_nodes, python);

    Ok(Dag::build(&all_nodes)?)
}

/// Recursively scan subdirectories for Python modules.
/// Stores dotted module paths as keys: `utils/math.py` → `"utils.math"`.
/// Handles `__init__.py`: `mylib/__init__.py` → `"mylib"`.
fn scan_subdirectories(
    dir: &std::path::Path,
    root: &std::path::Path,
    file_sources: &mut HashMap<String, String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let ep = entry.path();
        if ep.is_dir() {
            let dir_name = ep
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // Skip hidden dirs, __pycache__, .venv, etc.
            if dir_name.starts_with('.')
                || dir_name == "__pycache__"
                || dir_name == ".venv"
                || dir_name == "node_modules"
            {
                continue;
            }
            // Check if this is a Python package (has __init__.py).
            let init_path = ep.join("__init__.py");
            if init_path.exists() {
                if let Ok(content) = fs::read_to_string(&init_path) {
                    let module_path = ep
                        .strip_prefix(root)
                        .unwrap_or(&ep)
                        .to_string_lossy()
                        .replace(['/', '\\'], ".");
                    file_sources.entry(module_path).or_insert_with(|| content);
                }
            }
            // Scan .py files in the subdirectory.
            if let Ok(sub_entries) = std::fs::read_dir(&ep) {
                for sub_entry in sub_entries.flatten() {
                    let sp = sub_entry.path();
                    if sp.extension().map(|e| e == "py").unwrap_or(false)
                        && sp.file_name().map(|n| n != "__init__.py").unwrap_or(true)
                    {
                        if let Ok(content) = fs::read_to_string(&sp) {
                            // Build dotted module path relative to root.
                            let rel = sp.strip_prefix(root).unwrap_or(&sp);
                            let module_path = rel
                                .to_string_lossy()
                                .replace(['/', '\\'], ".")
                                .trim_end_matches(".py")
                                .to_string();
                            file_sources.entry(module_path).or_insert_with(|| content);
                        }
                    }
                }
            }
            // Recurse into deeper subdirectories.
            scan_subdirectories(&ep, root, file_sources);
        }
    }
}

fn resolve_dynamic_partitions(nodes: &mut [crate::model::ExtractedNode], python: &PathBuf) {
    for node in nodes.iter_mut() {
        let mut resolved: Vec<(String, Vec<crate::model::PartitionValue>)> = Vec::new();

        for (dim, spec) in &node.partitions {
            if let crate::model::PartitionSpec::Dynamic { source_text } = spec {
                let module_path = std::path::Path::new(&node.source_file)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&node.source_file));
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
                let partition_values: Vec<crate::model::PartitionValue> = values
                    .into_iter()
                    .filter_map(|v| match v {
                        serde_json::Value::String(s) => Some(crate::model::PartitionValue::Str(s)),
                        serde_json::Value::Number(n) => {
                            n.as_i64().map(crate::model::PartitionValue::Int)
                        }
                        _ => None,
                    })
                    .collect();

                resolved.push((dim.clone(), partition_values));
            }
        }

        for (dim, values) in resolved {
            node.partitions
                .insert(dim, crate::model::PartitionSpec::Static { values });
        }
    }
}
