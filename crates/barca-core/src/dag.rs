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
}

impl Dag {
    /// Build a DAG from extracted nodes. Validates all constraints.
    pub fn build(nodes: &[ExtractedNode]) -> Result<Self, DagError> {
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
        let Some(&target_idx) = self.index.get(target_id) else {
            return vec![];
        };

        // BFS backwards from target to find all ancestors.
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(target_idx);
        visited.insert(target_idx);

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

    /// Get upstream node IDs, excluding PartitionSource edges.
    /// Used by the planner for chain detection — partition source deps should
    /// force phase breaks, not chain bundling (and pass no data).
    pub fn execution_upstream(&self, id: &str) -> Vec<&str> {
        let Some(&idx) = self.index.get(id) else {
            return vec![];
        };
        let mut result = Vec::new();
        for edge in self.graph.edges_directed(idx, Direction::Incoming) {
            if !matches!(*edge.weight(), EdgeKind::PartitionSource) {
                let source_idx = edge.source();
                result.push(self.graph[source_idx].id.as_str());
            }
        }
        result
    }

    /// Get downstream node IDs, excluding PartitionSource edges.
    pub fn execution_downstream(&self, id: &str) -> Vec<&str> {
        let Some(&idx) = self.index.get(id) else {
            return vec![];
        };
        let mut result = Vec::new();
        for edge in self.graph.edges_directed(idx, Direction::Outgoing) {
            if !matches!(*edge.weight(), EdgeKind::PartitionSource) {
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
            parallel_calls: Vec::new(),
        }
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
