//! Phase execution engine — worker spawning, protocol parsing, partition expansion.

use barca_core::planner::{
    ExecutionPlan, Phase, StreamStep, WorkerStream, expand_partition_combos,
};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Dispatch all phases, collecting outputs. Returns node_id → output value.
pub fn dispatch_plan(
    plan: &ExecutionPlan,
    python: &PathBuf,
    _db_path: &str,
    pool_size: usize,
) -> HashMap<String, serde_json::Value> {
    let mut all_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    for phase in &plan.phases {
        let expanded_phase = expand_pending_partitions(phase, &all_outputs, pool_size);
        let phase_ref = expanded_phase.as_ref().unwrap_or(phase);

        let provided = build_provided_inputs(phase_ref, &all_outputs);
        let phase_outputs = execute_phase(phase_ref, &provided, python);

        for (node_id, value) in phase_outputs {
            all_outputs.insert(node_id, value);
        }
    }

    all_outputs
}

/// Expand steps with pending_partitions using materialized source outputs.
/// Returns None if no expansion needed, or a new Phase with expanded steps.
pub fn expand_pending_partitions(
    phase: &Phase,
    all_outputs: &HashMap<String, serde_json::Value>,
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
                let source_output = all_outputs
                    .iter()
                    .find(|(k, _)| k.ends_with(&format!(":{source_name}")))
                    .map(|(_, v)| v);

                if let Some(output) = source_output {
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

            let combos = expand_partition_combos(&dim_values);
            for combo in combos {
                let pk = barca_core::PartitionKey::from(combo);
                all_expanded_steps.push(StreamStep {
                    step_id: barca_core::StepId::new(step.step_id.base.clone(), pk),
                    kind: step.kind,
                    function_name: step.function_name.clone(),
                    source_file: step.source_file.clone(),
                    inputs: step.inputs.clone(),
                    pending_partitions: HashMap::new(),
                });
            }
        }
    }

    // Group expanded steps by partition key for aligned execution.
    let mut by_partition: HashMap<String, Vec<StreamStep>> = HashMap::new();
    for step in all_expanded_steps {
        let key = step.step_id.partition.suffix();
        by_partition.entry(key).or_default().push(step);
    }

    // Build work units: each partition group + passthrough steps.
    let mut work_units: Vec<Vec<StreamStep>> = Vec::new();
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
    let mut streams: Vec<Vec<StreamStep>> = vec![Vec::new(); num_streams];
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

/// Determine what values need to be provided to workers in this phase.
pub fn build_provided_inputs(
    phase: &Phase,
    all_outputs: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut provided: HashMap<String, serde_json::Value> = HashMap::new();

    for stream in &phase.streams {
        for step in &stream.steps {
            let suffix = if step.step_id.partition.is_empty() {
                String::new()
            } else {
                format!("[{}]", step.step_id.partition.suffix())
            };

            for upstream_id in step.inputs.values() {
                if let Some(output) = all_outputs.get(upstream_id) {
                    provided
                        .entry(upstream_id.clone())
                        .or_insert_with(|| output.clone());
                    continue;
                }
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
pub fn execute_phase(
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
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn worker: {e}"));

        children.push((child, path));
    }

    let mut phase_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    for (mut child, batch_path) in children {
        let stderr = child.stderr.take().expect("no stderr");
        let reader = BufReader::new(stderr);

        let (outputs, error_lines) = parse_worker_output(reader);
        for (node_id, output) in outputs {
            phase_outputs.insert(node_id, output);
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

const PROTOCOL_PREFIX: &str = "BARCA:1:";

/// Parse worker stderr output into protocol messages and error lines.
/// Protocol messages are prefixed with `BARCA:1:` followed by a JSON object.
pub fn parse_worker_output(
    reader: impl BufRead,
) -> (HashMap<String, serde_json::Value>, Vec<String>) {
    let mut outputs: HashMap<String, serde_json::Value> = HashMap::new();
    let mut error_lines: Vec<String> = Vec::new();

    for line in reader.lines() {
        let line = line.expect("failed to read worker stderr");
        if line.is_empty() {
            continue;
        }
        if let Some(json_str) = line.strip_prefix(PROTOCOL_PREFIX) {
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) else {
                eprintln!("[barca] malformed protocol message: {line}");
                continue;
            };
            match parsed.get("type").and_then(|v| v.as_str()) {
                Some("result") => {
                    if let (Some(node_id), Some(output)) = (
                        parsed.get("node_id").and_then(|v| v.as_str()),
                        parsed.get("output"),
                    ) {
                        outputs.insert(node_id.to_string(), output.clone());
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

/// Serialize a worker stream batch to JSON, including provided inputs.
pub fn serialize_batch(
    stream: &WorkerStream,
    provided_inputs: &HashMap<String, serde_json::Value>,
) -> String {
    let steps: Vec<serde_json::Value> = stream
        .steps
        .iter()
        .map(|s| {
            let inputs = if s.step_id.partition.is_empty() {
                s.inputs.clone()
            } else {
                let suffix = s.step_id.partition.suffix();
                s.inputs
                    .iter()
                    .map(|(param, upstream_id)| {
                        let aligned_id = format!("{upstream_id}[{suffix}]");
                        (param.clone(), aligned_id)
                    })
                    .collect()
            };

            let mut step = serde_json::json!({
                "node_id": s.step_id.display(),
                "kind": s.kind,
                "function_name": s.function_name,
                "source_file": s.source_file,
                "inputs": inputs,
            });
            if !s.step_id.partition.is_empty() {
                step["partition"] = serde_json::json!(s.step_id.partition.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use barca_core::planner::PhaseReason;

    #[test]
    fn parse_worker_output_separates_protocol_from_errors() {
        let input = "BARCA:1:{\"type\":\"result\",\"node_id\":\"test.py:foo\",\"output\":42,\"elapsed\":0.01}\n\
some error message\n\
BARCA:1:{\"type\":\"result\",\"node_id\":\"test.py:bar\",\"output\":{\"x\":1},\"elapsed\":0.02}\n\
Traceback (most recent call last):\n\
  File \"test.py\", line 5\n";
        let reader = std::io::Cursor::new(input);
        let (outputs, errors) = parse_worker_output(reader);

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs["test.py:foo"], serde_json::json!(42));
        assert_eq!(outputs["test.py:bar"], serde_json::json!({"x": 1}));
        assert_eq!(errors.len(), 3);
        assert!(errors[0].contains("some error message"));
    }

    #[test]
    fn parse_worker_output_ignores_empty_lines() {
        let input = "\n\nBARCA:1:{\"type\":\"result\",\"node_id\":\"a\",\"output\":1}\n\n";
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
        let input = "BARCA:1:{\"type\":\"progress\",\"node_id\":\"a\",\"pct\":50}\n";
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

    use barca_core::{NodeKind, PartitionKey, StepId};

    #[test]
    fn build_provided_inputs_direct_match() {
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:b"),
                    kind: NodeKind::Asset,
                    function_name: "b".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::from([("a_val".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                }],
            }],
        };
        let mut all_outputs = HashMap::new();
        all_outputs.insert("f:a".to_string(), serde_json::json!(10));

        let provided = build_provided_inputs(&phase, &all_outputs);
        assert_eq!(provided["f:a"], serde_json::json!(10));
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
                    function_name: "b".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::from([("a_val".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                }],
            }],
        };
        let mut all_outputs = HashMap::new();
        all_outputs.insert("f:a[t=X]".to_string(), serde_json::json!(99));

        let provided = build_provided_inputs(&phase, &all_outputs);
        assert_eq!(provided["f:a[t=X]"], serde_json::json!(99));
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
                    function_name: "a".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::new(),
                }],
            }],
        };
        assert!(expand_pending_partitions(&phase, &HashMap::new(), 4).is_none());
    }

    #[test]
    fn expand_pending_partitions_expands_derived() {
        let phase = Phase {
            reason: PhaseReason::Initial,
            streams: vec![WorkerStream {
                stream_id: "w0".to_string(),
                steps: vec![StreamStep {
                    step_id: StepId::unpartitioned("f:transform"),
                    kind: NodeKind::Asset,
                    function_name: "transform".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::from([(
                        "region".to_string(),
                        "get_regions".to_string(),
                    )]),
                }],
            }],
        };
        let mut outputs = HashMap::new();
        outputs.insert("f:get_regions".to_string(), serde_json::json!(["us", "eu"]));

        let expanded = expand_pending_partitions(&phase, &outputs, 4).unwrap();
        let all_steps: Vec<String> = expanded
            .streams
            .iter()
            .flat_map(|s| s.steps.iter().map(|st| st.step_id.display()))
            .collect();
        assert!(all_steps.contains(&"f:transform[region=eu]".to_string()));
        assert!(all_steps.contains(&"f:transform[region=us]".to_string()));
        assert_eq!(all_steps.len(), 2);
    }

    #[test]
    fn serialize_batch_includes_partition_aligned_inputs() {
        let pk = PartitionKey::from(HashMap::from([("t".to_string(), "X".to_string())]));
        let stream = WorkerStream {
            stream_id: "w0".to_string(),
            steps: vec![StreamStep {
                step_id: StepId::new("f:b", pk),
                kind: NodeKind::Asset,
                function_name: "b".to_string(),
                source_file: "f".to_string(),
                inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                pending_partitions: HashMap::new(),
            }],
        };

        let json_str = serialize_batch(&stream, &HashMap::new());
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let step_inputs = &parsed["steps"][0]["inputs"];
        assert_eq!(step_inputs["data"], "f:a[t=X]");
    }
}
