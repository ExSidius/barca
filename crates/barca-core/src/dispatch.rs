//! Phase execution engine — worker spawning, protocol parsing, partition expansion.

use crate::planner::{ExecutionPlan, Phase, StreamStep, WorkerStream, expand_partition_combos};
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
}

/// Result of dispatching all phases — includes partial outputs if a phase failed.
pub struct DispatchResult {
    pub outputs: HashMap<String, OutputRef>,
    pub error: Option<String>,
}

/// Dispatch all phases, collecting outputs. Returns partial results even on failure.
pub fn dispatch_plan(
    plan: &ExecutionPlan,
    python: &PathBuf,
    _db_path: &str,
    pool_size: usize,
) -> DispatchResult {
    let mut all_outputs: HashMap<String, OutputRef> = HashMap::new();

    for phase in &plan.phases {
        let expanded_phase = expand_pending_partitions(phase, &all_outputs, pool_size);
        let phase_ref = expanded_phase.as_ref().unwrap_or(phase);

        let provided = build_provided_inputs(phase_ref, &all_outputs);
        let result = execute_phase(phase_ref, &provided, python);

        for (node_id, value) in result.outputs {
            all_outputs.insert(node_id, value);
        }

        if let Some(error) = result.error {
            return DispatchResult {
                outputs: all_outputs,
                error: Some(error),
            };
        }
    }

    DispatchResult {
        outputs: all_outputs,
        error: None,
    }
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
            for combo in combos {
                let pk = crate::PartitionKey::from(combo);
                all_expanded_steps.push(StreamStep {
                    step_id: crate::StepId::new(step.step_id.base.clone(), pk),
                    kind: step.kind,
                    function_name: step.function_name.clone(),
                    source_file: step.source_file.clone(),
                    inputs: step.inputs.clone(),
                    pending_partitions: HashMap::new(),
                    serializer: step.serializer.clone(),
                    timeout_seconds: step.timeout_seconds,
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

/// A provided input — either a single artifact or a collected list of partition artifacts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProvidedInput {
    Single(OutputRef),
    Collected(Vec<OutputRef>),
}

/// Determine what values need to be provided to workers in this phase.
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
                if let Some(output) = all_outputs.get(upstream_id) {
                    provided
                        .entry(upstream_id.clone())
                        .or_insert_with(|| ProvidedInput::Single(output.clone()));
                    continue;
                }
                if !suffix.is_empty() {
                    let aligned_id = format!("{upstream_id}{suffix}");
                    if let Some(output) = all_outputs.get(&aligned_id) {
                        provided
                            .entry(aligned_id)
                            .or_insert_with(|| ProvidedInput::Single(output.clone()));
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

/// Execute a single phase: spawn N workers in parallel, collect results from stderr.
/// Result of executing a phase — may contain partial outputs if a worker failed.
pub struct PhaseResult {
    pub outputs: HashMap<String, OutputRef>,
    pub error: Option<String>,
}

pub fn execute_phase(
    phase: &Phase,
    provided_inputs: &HashMap<String, ProvidedInput>,
    python: &PathBuf,
) -> PhaseResult {
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

    // Drain stderr from all workers in parallel via reader threads to avoid
    // 64KB pipe buffer deadlocks when workers produce significant output.
    let mut handles: Vec<(
        std::thread::JoinHandle<(HashMap<String, OutputRef>, Vec<String>)>,
        std::process::Child,
        PathBuf,
    )> = Vec::new();

    for (mut child, batch_path) in children {
        let stderr = child.stderr.take().expect("no stderr");
        let handle = std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            parse_worker_output(reader)
        });
        handles.push((handle, child, batch_path));
    }

    let mut phase_outputs: HashMap<String, OutputRef> = HashMap::new();
    let mut first_error: Option<String> = None;

    for (handle, mut child, batch_path) in handles {
        let (outputs, error_lines) = handle.join().expect("reader thread panicked");
        // Always collect outputs — even from failed workers, partial results are valuable.
        for (node_id, output) in outputs {
            phase_outputs.insert(node_id, output);
        }

        let status = child.wait().expect("failed to wait on worker");
        if !status.success() && first_error.is_none() {
            first_error = Some(filter_traceback(&error_lines));
        }
        std::fs::remove_file(&batch_path).ok();
    }

    PhaseResult {
        outputs: phase_outputs,
        error: first_error,
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
                    function_name: "b".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::from([("a_val".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
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
                    function_name: "b".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::from([("a_val".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
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
                    function_name: "a".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
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
                    function_name: "transform".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::from([(
                        "region".to_string(),
                        "get_regions".to_string(),
                    )]),
                    serializer: None,
                    timeout_seconds: 300,
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
            },
        );

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
                serializer: None,
                timeout_seconds: 300,
            }],
        };

        let json_str = serialize_batch(&stream, &HashMap::<String, ProvidedInput>::new());
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let step_inputs = &parsed["steps"][0]["inputs"];
        assert_eq!(step_inputs["data"], "f:a[t=X]");
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
                    function_name: "b".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
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
                    function_name: "b".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                    pending_partitions: HashMap::new(),
                    serializer: None,
                    timeout_seconds: 300,
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
                function_name: "b".to_string(),
                source_file: "f".to_string(),
                inputs: HashMap::from([("data".to_string(), "f:a".to_string())]),
                pending_partitions: HashMap::new(),
                serializer: None,
                timeout_seconds: 300,
            }],
        };
        let mut provided: HashMap<String, ProvidedInput> = HashMap::new();
        provided.insert(
            "f:a".to_string(),
            ProvidedInput::Single(OutputRef {
                path: ".barca/artifacts/f--a.json".to_string(),
                format: "json".to_string(),
                size_bytes: 100,
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
                function_name: "a".to_string(),
                source_file: "f".to_string(),
                inputs: HashMap::new(),
                pending_partitions: HashMap::new(),
                serializer: None,
                timeout_seconds: 300,
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
                    function_name: "transform".to_string(),
                    source_file: "f".to_string(),
                    inputs: HashMap::new(),
                    pending_partitions: HashMap::from([(
                        "region".to_string(),
                        "get_regions".to_string(),
                    )]),
                    serializer: None,
                    timeout_seconds: 300,
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
            },
        );

        let expanded = expand_pending_partitions(&phase, &outputs, 4).unwrap();
        let all_steps: Vec<String> = expanded
            .streams
            .iter()
            .flat_map(|s| s.steps.iter().map(|st| st.step_id.display()))
            .collect();
        assert!(all_steps.contains(&"f:transform[region=us]".to_string()));
        assert!(all_steps.contains(&"f:transform[region=eu]".to_string()));
        assert!(all_steps.contains(&"f:transform[region=ap]".to_string()));
        assert_eq!(all_steps.len(), 3);
    }
}
