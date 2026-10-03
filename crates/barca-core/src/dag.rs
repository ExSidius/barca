//! DAG construction and query — builds a directed acyclic graph from extracted
//! nodes, validates constraints, and supports traversal operations.

use petgraph::Direction;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

use crate::hash;
use crate::model::{DagNode, EdgeKind, ExtractedNode, NodeKind};

/// The constructed DAG — validated, acyclic, ready for plan generation.
#[derive(Debug)]
pub struct Dag {
    pub graph: DiGraph<DagNode, EdgeKind>,
    index: HashMap<String, NodeIndex>,
}

#[derive(Debug, thiserror::Error)]
pub enum DagError {
    #[error("cycle detected in dependency graph")]
    CycleDetected,

    #[error("input '{param}' on node '{node}' references unknown upstream '{upstream}'")]
    UnresolvedInput {
        node: String,
        param: String,
        upstream: String,
    },

    #[error(
        "task '{task}' cannot be an input to {downstream_kind} '{downstream}' \
         (tasks are never cached, so this would poison caching)"
    )]
    TaskAsInput {
        task: String,
        downstream: String,
        downstream_kind: &'static str,
    },

    #[error("duplicate continuity key: '{key}' defined in both '{first}' and '{second}'")]
    DuplicateKey {
        key: String,
        first: String,
        second: String,
    },

    #[error("sensor '{sensor}' cannot have inputs")]
    SensorWithInputs { sensor: String },

    /// `inputs={"x": upstream}` where `upstream` is partitioned and the consumer is not (#189).
    /// This used to pass every partition as a list, a second spelling of `collect(upstream)`.
    #[error(
        "input '{param}' on '{node}' reads partitioned asset '{upstream}', but '{node}' is not \
         partitioned"
    )]
    PartitionedInputToUnpartitioned {
        node: String,
        param: String,
        upstream: String,
    },

    /// `partitions_from(upstream)` on a partitioned upstream that the consumer cannot mirror.
    #[error("'{node}': partitions_from({upstream}) {problem}")]
    PartitionsFrom {
        node: String,
        upstream: String,
        problem: String,
        fix: String,
    },
}

impl DagError {
    /// The fix, when this error has one more specific than "check each node's inputs".
    pub fn remediation(&self) -> Option<String> {
        match self {
            DagError::PartitionedInputToUnpartitioned {
                param, upstream, ..
            } => {
                let name = upstream.rsplit(':').next().unwrap_or(upstream);
                Some(format!(
                    "Use `inputs={{\"{param}\": collect({name})}}` to receive every partition of \
                     '{name}' as one list, or `partitions={{\"<key>\": partitions_from({name})}}` \
                     to run once per partition of '{name}' with that partition's output."
                ))
            }
            DagError::PartitionsFrom { fix, .. } => Some(fix.clone()),
            _ => None,
        }
    }
}

/// Resolve `partitions_from(upstream)` where `upstream` is itself partitioned (#189).
///
/// The consumer takes the upstream's partition spec (so the same keys, static or resolved at
/// run time), and each key reads that key of the upstream under the upstream's name, unless an
/// `inputs=` entry already names the upstream (then it arrives under that parameter). Both
/// steps are then partitioned by the same dimension, so the coordinator wires each consumer key
/// to the producer key with the same partition key, as for any partition-aligned input.
///
/// `partitions_from(<unpartitioned asset>)` is left alone: its keys are the values of the list
/// that asset returns, read at dispatch time (`dispatch::expand_pending_partitions`).
fn resolve_partitions_from(nodes: &[ExtractedNode]) -> Result<Vec<ExtractedNode>, DagError> {
    use crate::model::{DeclaredInput, NodeRef, PartitionSpec};

    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Todo,
        Active,
        Done,
    }

    fn resolve(
        i: usize,
        nodes: &mut [ExtractedNode],
        by_name: &HashMap<String, usize>,
        state: &mut [State],
    ) -> Result<(), DagError> {
        if state[i] != State::Todo {
            return Ok(()); // done, or a cycle (reported by `Dag::build`)
        }
        state[i] = State::Active;
        let derived: Vec<(String, String)> = nodes[i]
            .partitions
            .iter()
            .filter_map(|(dim, spec)| match spec {
                PartitionSpec::DerivedFrom { source_ref } => {
                    Some((dim.clone(), source_ref.resolution_name().to_string()))
                }
                _ => None,
            })
            .collect();
        for (dim, source) in derived {
            let Some(&j) = by_name.get(&source) else {
                continue;
            };
            resolve(j, nodes, by_name, state)?;
            if nodes[j].partitions.is_empty() {
                continue; // keys come from the list the source returns
            }
            let node = nodes[i].continuity_key();
            let err = |problem: String, fix: String| DagError::PartitionsFrom {
                node: node.clone(),
                upstream: source.clone(),
                problem,
                fix,
            };
            if nodes[j].partitions.len() != 1 {
                let mut dims: Vec<&String> = nodes[j].partitions.keys().collect();
                dims.sort();
                return Err(err(
                    format!(
                        "is not supported: '{source}' has {} partition dimensions ({})",
                        dims.len(),
                        dims.iter()
                            .map(|d| d.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    format!(
                        "Declare the partitions of '{}' explicitly with partitions([...]).",
                        nodes[i].function_name
                    ),
                ));
            }
            let (src_dim, src_spec) = nodes[j]
                .partitions
                .iter()
                .next()
                .map(|(d, s)| (d.clone(), s.clone()))
                .expect("one dimension");
            if src_dim != dim {
                return Err(err(
                    format!(
                        "is declared under dimension '{dim}', but '{source}' is partitioned by \
                         '{src_dim}'"
                    ),
                    format!(
                        "Use the upstream's dimension name: \
                         `partitions={{\"{src_dim}\": partitions_from({source})}}`, with a \
                         parameter named '{src_dim}'."
                    ),
                ));
            }
            if nodes[i].partitions.len() != 1 {
                return Err(err(
                    "must be the asset's only partition dimension".to_string(),
                    format!(
                        "Remove the other dimensions from '{}', or declare its keys explicitly \
                         with partitions([...]).",
                        nodes[i].function_name
                    ),
                ));
            }
            let consumer = &mut nodes[i];
            consumer.partitions.insert(dim, src_spec);
            let named = consumer
                .inputs
                .iter()
                .any(|inp| !inp.collected && inp.upstream.resolution_name() == source);
            if !named {
                consumer.inputs.push(DeclaredInput {
                    param_name: source.clone(),
                    upstream: NodeRef::FunctionName(source.clone()),
                    collected: false,
                });
            }
        }
        state[i] = State::Done;
        Ok(())
    }

    let mut out = nodes.to_vec();
    let by_name: HashMap<String, usize> = out
        .iter()
        .enumerate()
        .map(|(i, n)| (n.function_name.clone(), i))
        .collect();
    let mut state = vec![State::Todo; out.len()];
    for i in 0..out.len() {
        resolve(i, &mut out, &by_name, &mut state)?;
    }
    Ok(out)
}

impl Dag {
    /// Build a DAG from extracted nodes. Validates all constraints.
    pub fn build(nodes: &[ExtractedNode]) -> Result<Self, DagError> {
        let resolved = resolve_partitions_from(nodes)?;
        let nodes = resolved.as_slice();
        let mut graph = DiGraph::new();
        let mut index: HashMap<String, NodeIndex> = HashMap::new();

        // Track function_name → continuity_key for resolution.
        let mut name_to_key: HashMap<String, String> = HashMap::new();

        // First pass: add all nodes, check for duplicate keys.
        for node in nodes {
            let id = node.continuity_key();

            if let Some(existing_idx) = index.get(&id) {
                let existing: &DagNode = &graph[*existing_idx];
                return Err(DagError::DuplicateKey {
                    key: id,
                    first: existing.source_file().to_string(),
                    second: node.source_file.clone(),
                });
            }

            // Validate: sensors cannot have inputs.
            if node.kind == NodeKind::Sensor && !node.inputs.is_empty() {
                return Err(DagError::SensorWithInputs { sensor: id.clone() });
            }

            // Compute definition_hash from source text + dependency cone + metadata.
            // Sinks and serializer are included so that changing a @sink or
            // @asset(serializer=) re-materializes the node — cached steps never
            // reach a worker, so their sinks would otherwise silently not run.
            let metadata = serde_json::json!({
                "kind": node.kind,
                "freshness": node.freshness,
                "inputs": node.inputs.iter().map(|i| &i.param_name).collect::<Vec<_>>(),
                "sinks": node.sinks,
                "serializer": node.artifact_serializer,
            })
            .to_string();
            let def_hash = hash::definition_hash(&node.source_text, &node.cone_hash, &metadata);

            let dag_node = DagNode {
                id: id.clone(),
                extracted: node.clone(),
                resolved_inputs: HashMap::new(),
                resolved_collected: HashMap::new(),
                definition_hash: def_hash,
            };

            let idx = graph.add_node(dag_node);
            index.insert(id.clone(), idx);
            name_to_key.insert(node.function_name.clone(), id);
        }

        // Second pass: add edges, resolve inputs.
        for node in nodes {
            let downstream_key = node.continuity_key();
            let downstream_idx = index[&downstream_key];

            for input in &node.inputs {
                let upstream_name = input.upstream.resolution_name();
                let Some(upstream_key) = name_to_key.get(upstream_name) else {
                    return Err(DagError::UnresolvedInput {
                        node: downstream_key.clone(),
                        param: input.param_name.clone(),
                        upstream: upstream_name.to_string(),
                    });
                };

                let upstream_idx = index[upstream_key.as_str()];

                // Validate: tasks cannot be an input to an asset or sensor.
                // (Tasks always re-run and never cache, so feeding a task's output
                // into a cacheable node would make that node perpetually stale.)
                if graph[upstream_idx].kind() == NodeKind::Task {
                    let downstream_kind = match node.kind {
                        NodeKind::Asset => Some("asset"),
                        NodeKind::Sensor => Some("sensor"),
                        NodeKind::Task => None,
                    };
                    if let Some(downstream_kind) = downstream_kind {
                        return Err(DagError::TaskAsInput {
                            task: upstream_key.clone(),
                            downstream: downstream_key.clone(),
                            downstream_kind,
                        });
                    }
                }

                // A partitioned upstream read whole by an unpartitioned consumer: say which of
                // the two meanings was intended instead of guessing one (#189).
                if !input.collected
                    && node.partitions.is_empty()
                    && !graph[upstream_idx].extracted.partitions.is_empty()
                {
                    return Err(DagError::PartitionedInputToUnpartitioned {
                        node: downstream_key.clone(),
                        param: input.param_name.clone(),
                        upstream: upstream_key.clone(),
                    });
                }

                let edge_kind = if input.collected {
                    EdgeKind::Collect
                } else {
                    EdgeKind::Direct
                };
                graph.add_edge(upstream_idx, downstream_idx, edge_kind);

                // Record the resolved mapping on the node.
                if input.collected {
                    graph[downstream_idx]
                        .resolved_collected
                        .insert(input.param_name.clone(), upstream_key.clone());
                } else {
                    graph[downstream_idx]
                        .resolved_inputs
                        .insert(input.param_name.clone(), upstream_key.clone());
                }
            }

            // Add partition_source edges for partitions_from.
            for spec in node.partitions.values() {
                if let crate::model::PartitionSpec::DerivedFrom { source_ref } = spec {
                    let source_name = source_ref.resolution_name();
                    if let Some(source_key) = name_to_key.get(source_name) {
                        let source_idx = index[source_key.as_str()];
                        graph.add_edge(source_idx, downstream_idx, EdgeKind::PartitionSource);
                    }
                }
            }
        }

        let dag = Dag { graph, index };

        // Verify acyclicity.
        if toposort(&dag.graph, None).is_err() {
            return Err(DagError::CycleDetected);
        }

        Ok(dag)
    }

    /// Get the subgraph of all nodes upstream of (and including) target.
    /// Returns node IDs in topological order (dependencies first).
    pub fn subgraph(&self, target_id: &str) -> Vec<&str> {
        self.subgraph_many(&[target_id])
    }

    /// The union of the subgraphs of several targets: every node upstream of (and including)
    /// any of them, each once, in topological order. Unknown ids are ignored.
    pub fn subgraph_many(&self, target_ids: &[&str]) -> Vec<&str> {
        // BFS backwards from the targets to find all ancestors.
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        for id in target_ids {
            if let Some(&idx) = self.index.get(*id)
                && visited.insert(idx)
            {
                queue.push_back(idx);
            }
        }
        if queue.is_empty() {
            return vec![];
        }

        while let Some(idx) = queue.pop_front() {
            for pred in self.graph.neighbors_directed(idx, Direction::Incoming) {
                if visited.insert(pred) {
                    queue.push_back(pred);
                }
            }
        }

        // Return in topo order (filtered to subgraph).
        let sorted = toposort(&self.graph, None).expect("verified acyclic");
        sorted
            .into_iter()
            .filter(|idx| visited.contains(idx))
            .map(|idx| self.graph[idx].id.as_str())
            .collect()
    }

    /// Topologically sorted node IDs.
    pub fn topo_order(&self) -> Vec<&str> {
        let sorted = toposort(&self.graph, None).expect("verified acyclic");
        sorted
            .iter()
            .map(|idx| self.graph[*idx].id.as_str())
            .collect()
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&DagNode> {
        self.index.get(id).map(|idx| &self.graph[*idx])
    }

    /// Get upstream node IDs.
    pub fn upstream(&self, id: &str) -> Vec<&str> {
        let Some(&idx) = self.index.get(id) else {
            return vec![];
        };
        self.graph
            .neighbors_directed(idx, Direction::Incoming)
            .map(|pred| self.graph[pred].id.as_str())
            .collect()
    }

    /// Get downstream node IDs.
    pub fn downstream(&self, id: &str) -> Vec<&str> {
        let Some(&idx) = self.index.get(id) else {
            return vec![];
        };
        self.graph
            .neighbors_directed(idx, Direction::Outgoing)
            .map(|succ| self.graph[succ].id.as_str())
            .collect()
    }

    /// Get upstream node IDs, excluding PartitionSource and Collect edges.
    /// Used by the planner for chain detection — partition source deps should
    /// force phase breaks, not chain bundling (and pass no data). Collect
    /// (fan-in) deps must also force a phase break: a `collect()` consumer
    /// has to wait for *every* partition of its upstream to finish, which a
    /// fused single-succ/single-pred chain cannot guarantee (see #97).
    pub fn execution_upstream(&self, id: &str) -> Vec<&str> {
        let Some(&idx) = self.index.get(id) else {
            return vec![];
        };
        let mut result = Vec::new();
        for edge in self.graph.edges_directed(idx, Direction::Incoming) {
            if !matches!(
                *edge.weight(),
                EdgeKind::PartitionSource | EdgeKind::Collect
            ) {
                let source_idx = edge.source();
                result.push(self.graph[source_idx].id.as_str());
            }
        }
        result
    }

    /// Get downstream node IDs, excluding PartitionSource and Collect edges.
    pub fn execution_downstream(&self, id: &str) -> Vec<&str> {
        let Some(&idx) = self.index.get(id) else {
            return vec![];
        };
        let mut result = Vec::new();
        for edge in self.graph.edges_directed(idx, Direction::Outgoing) {
            if !matches!(
                *edge.weight(),
                EdgeKind::PartitionSource | EdgeKind::Collect
            ) {
                let target_idx = edge.target();
                result.push(self.graph[target_idx].id.as_str());
            }
        }
        result
    }

    /// Node count.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Edge count.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SerializerKind, SinkDecl};

    fn node(name: &str) -> ExtractedNode {
        ExtractedNode {
            kind: NodeKind::Asset,
            function_name: name.to_string(),
            explicit_name: None,
            freshness: crate::model::Freshness::Always,
            inputs: smallvec::SmallVec::new(),
            partitions: HashMap::new(),
            sinks: smallvec::SmallVec::new(),
            timeout_seconds: 300,
            retries: 1,
            retry_backoff_seconds: 0.0,
            description: None,
            tags: HashMap::new(),
            is_unsafe: false,
            source_file: "test.py".to_string(),
            byte_offset: 0,
            source_text: "def a(): return 1".to_string(),
            cone_hash: String::new(),
            artifact_serializer: None,
            param_types: HashMap::new(),
            return_type: None,
            parallel_calls: Vec::new(),
            env: Vec::new(),
        }
    }

    fn build_src(src: &str) -> Result<Dag, DagError> {
        Dag::build(&crate::parse::extract_nodes(src, "t.py").unwrap())
    }

    const SALES: &str = "from barca import asset, collect, partitions, partitions_from\n\n\
@asset(partitions={\"region\": partitions([\"emea\", \"amer\"])})\n\
def sales(region: str) -> dict:\n    return {}\n\n";

    #[test]
    fn partitions_from_a_partitioned_asset_takes_its_keys_and_reads_it_per_key() {
        let dag = build_src(&format!(
            "{SALES}@asset(partitions={{\"region\": partitions_from(sales)}})\n\
def margin(region: str, sales: dict) -> dict:\n    return sales\n\n\
@asset(partitions={{\"region\": partitions_from(margin)}})\n\
def pct(region: str, margin: dict) -> dict:\n    return margin\n"
        ))
        .unwrap();
        let sales = &dag.get_node("t.py:sales").unwrap().extracted.partitions;
        for (id, upstream) in [("t.py:margin", "sales"), ("t.py:pct", "margin")] {
            let n = dag.get_node(id).unwrap();
            assert_eq!(&n.extracted.partitions, sales, "{id} has the keys of sales");
            assert_eq!(
                n.resolved_inputs.get(upstream).map(String::as_str),
                Some(format!("t.py:{upstream}").as_str()),
                "{id} reads {upstream} under its name"
            );
        }
    }

    #[test]
    fn partitions_from_an_inputs_entry_names_the_parameter() {
        let dag = build_src(&format!(
            "{SALES}@asset(inputs={{\"s\": sales}}, partitions={{\"region\": partitions_from(sales)}})\n\
def margin(region: str, s: dict) -> dict:\n    return s\n"
        ))
        .unwrap();
        let n = dag.get_node("t.py:margin").unwrap();
        assert_eq!(n.resolved_inputs.len(), 1);
        assert_eq!(n.resolved_inputs["s"], "t.py:sales");
    }

    #[test]
    fn partitions_from_a_list_asset_is_unchanged() {
        let dag = build_src(
            "from barca import asset, partitions_from\n\n\
@asset()\ndef keys() -> list:\n    return []\n\n\
@asset(partitions={\"k\": partitions_from(keys)})\ndef part(k: str) -> str:\n    return k\n",
        )
        .unwrap();
        let n = dag.get_node("t.py:part").unwrap();
        assert!(n.resolved_inputs.is_empty());
        assert!(matches!(
            n.extracted.partitions["k"],
            crate::model::PartitionSpec::DerivedFrom { .. }
        ));
    }

    #[test]
    fn partitions_from_under_another_dimension_name_is_an_error() {
        let err = build_src(&format!(
            "{SALES}@asset(partitions={{\"r\": partitions_from(sales)}})\n\
def margin(r: str, sales: dict) -> dict:\n    return sales\n"
        ))
        .unwrap_err();
        assert!(matches!(err, DagError::PartitionsFrom { .. }), "{err}");
        assert!(err.to_string().contains("'region'"), "{err}");
        assert!(
            err.remediation()
                .unwrap()
                .contains("partitions_from(sales)")
        );
    }

    #[test]
    fn partitions_from_needs_one_dimension_on_both_sides() {
        let several = build_src(
            "from barca import asset, partitions, partitions_from\n\n\
@asset(partitions={\"a\": partitions([\"x\"]), \"b\": partitions([\"y\"])})\n\
def grid(a: str, b: str) -> str:\n    return a\n\n\
@asset(partitions={\"a\": partitions_from(grid)})\n\
def down(a: str, grid: str) -> str:\n    return grid\n",
        )
        .unwrap_err();
        assert!(
            several
                .to_string()
                .contains("2 partition dimensions (a, b)"),
            "{several}"
        );
        let mixed = build_src(&format!(
            "{SALES}@asset(partitions={{\"region\": partitions_from(sales), \"tier\": partitions([\"1\"])}})\n\
def margin(region: str, tier: str, sales: dict) -> dict:\n    return sales\n"
        ))
        .unwrap_err();
        assert!(
            mixed.to_string().contains("only partition dimension"),
            "{mixed}"
        );
    }

    #[test]
    fn partitioned_input_to_unpartitioned_consumer_is_an_error() {
        let err = build_src(&format!(
            "{SALES}@asset(inputs={{\"xs\": sales}})\ndef total(xs: list) -> int:\n    return 0\n"
        ))
        .unwrap_err();
        assert!(
            matches!(err, DagError::PartitionedInputToUnpartitioned { .. }),
            "{err}"
        );
        let fix = err.remediation().unwrap();
        assert!(
            fix.contains("collect(sales)") && fix.contains("partitions_from(sales)"),
            "{fix}"
        );
        // collect() is the way to read every partition.
        build_src(&format!(
            "{SALES}@asset(inputs={{\"xs\": collect(sales)}})\ndef total(xs: list) -> int:\n    return 0\n"
        ))
        .unwrap();
    }

    #[test]
    fn subgraph_many_is_the_union_of_cones_with_shared_upstream_once() {
        let src = "from barca import asset\n\n\
@asset()\ndef src() -> int:\n    return 1\n\n\
@asset(inputs={\"s\": src})\ndef a(s: int) -> int:\n    return s\n\n\
@asset(inputs={\"s\": src})\ndef b(s: int) -> int:\n    return s\n\n\
@asset()\ndef other() -> int:\n    return 2\n";
        let nodes = crate::parse::extract_nodes(src, "t.py").unwrap();
        let dag = Dag::build(&nodes).unwrap();
        let union = dag.subgraph_many(&["t.py:a", "t.py:b"]);
        assert_eq!(union.len(), 3);
        assert_eq!(union[0], "t.py:src", "dependencies come first");
        assert!(union.contains(&"t.py:a") && union.contains(&"t.py:b"));
        assert!(!union.contains(&"t.py:other"));
        assert_eq!(dag.subgraph("t.py:a"), dag.subgraph_many(&["t.py:a"]));
        assert!(dag.subgraph_many(&["t.py:nope"]).is_empty());
    }

    #[test]
    fn definition_hash_changes_when_sink_added() {
        let plain = node("a");
        let mut sinked = node("a");
        sinked.sinks.push(SinkDecl {
            path: "exports/a.parquet".to_string(),
            serializer: None,
        });

        let dag_plain = Dag::build(std::slice::from_ref(&plain)).unwrap();
        let dag_sinked = Dag::build(std::slice::from_ref(&sinked)).unwrap();
        assert_ne!(
            dag_plain.get_node("test.py:a").unwrap().definition_hash,
            dag_sinked.get_node("test.py:a").unwrap().definition_hash,
        );
    }

    #[test]
    fn definition_hash_changes_when_sink_edited() {
        let mut s1 = node("a");
        s1.sinks.push(SinkDecl {
            path: "exports/a.parquet".to_string(),
            serializer: None,
        });
        let mut s2 = node("a");
        s2.sinks.push(SinkDecl {
            path: "exports/a.parquet".to_string(),
            serializer: Some(SerializerKind::Pickle),
        });

        let d1 = Dag::build(std::slice::from_ref(&s1)).unwrap();
        let d2 = Dag::build(std::slice::from_ref(&s2)).unwrap();
        assert_ne!(
            d1.get_node("test.py:a").unwrap().definition_hash,
            d2.get_node("test.py:a").unwrap().definition_hash,
        );
    }

    #[test]
    fn definition_hash_changes_when_serializer_changed() {
        let plain = node("a");
        let mut with_ser = node("a");
        with_ser.artifact_serializer = Some(SerializerKind::Parquet);

        let d1 = Dag::build(std::slice::from_ref(&plain)).unwrap();
        let d2 = Dag::build(std::slice::from_ref(&with_ser)).unwrap();
        assert_ne!(
            d1.get_node("test.py:a").unwrap().definition_hash,
            d2.get_node("test.py:a").unwrap().definition_hash,
        );
    }
}
