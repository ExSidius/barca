//! DAG construction and query — builds a directed acyclic graph from extracted
//! nodes, validates constraints, and supports traversal operations.

use petgraph::Direction;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

use crate::model::{DagNode, EdgeKind, ExtractedNode, NodeKind};
use crate::plan::{DagShape, KindBreakdown, PlanStats};

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

    #[error("effect '{effect}' cannot be used as input to '{downstream}'")]
    EffectAsInput { effect: String, downstream: String },

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
                    first: existing.source_file.clone(),
                    second: node.source_file.clone(),
                });
            }

            // Validate: sensors cannot have inputs.
            if node.kind == NodeKind::Sensor && !node.inputs.is_empty() {
                return Err(DagError::SensorWithInputs { sensor: id.clone() });
            }

            let dag_node = DagNode {
                id: id.clone(),
                kind: node.kind,
                function_name: node.function_name.clone(),
                source_file: node.source_file.clone(),
                freshness: node.freshness.clone(),
                inputs: HashMap::new(),
                collected_inputs: HashMap::new(),
                partition_keys: node.partitions.keys().cloned().collect(),
                partition_specs: node.partitions.clone(),
                sinks: node.sinks.clone(),
                timeout_seconds: node.timeout_seconds,
                tags: node.tags.clone(),
                is_unsafe: node.is_unsafe,
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

                // Validate: effects cannot be inputs.
                if graph[upstream_idx].kind == NodeKind::Effect {
                    return Err(DagError::EffectAsInput {
                        effect: upstream_key.clone(),
                        downstream: downstream_key.clone(),
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
                        .collected_inputs
                        .insert(input.param_name.clone(), upstream_key.clone());
                } else {
                    graph[downstream_idx]
                        .inputs
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

    /// Topologically sorted node IDs.
    pub fn topo_order(&self) -> Vec<&str> {
        let sorted = toposort(&self.graph, None).expect("verified acyclic");
        sorted
            .iter()
            .map(|idx| self.graph[*idx].id.as_str())
            .collect()
    }

    /// Compute parallelism tiers: tier[i] = max(tier[predecessors]) + 1.
    pub fn compute_tiers(&self) -> HashMap<String, usize> {
        let sorted = toposort(&self.graph, None).expect("verified acyclic");
        let mut tiers: HashMap<NodeIndex, usize> = HashMap::new();

        for &idx in &sorted {
            let tier = self
                .graph
                .neighbors_directed(idx, Direction::Incoming)
                .map(|pred| tiers[&pred] + 1)
                .max()
                .unwrap_or(0);
            tiers.insert(idx, tier);
        }

        tiers
            .into_iter()
            .map(|(idx, tier)| (self.graph[idx].id.clone(), tier))
            .collect()
    }

    /// Classify the DAG shape for optimization purposes.
    pub fn classify_shape(&self) -> DagShape {
        let n = self.graph.node_count();
        let e = self.graph.edge_count();

        if n == 0 {
            return DagShape::LinearChain;
        }

        // Linear chain: N nodes, N-1 edges, max in-degree and out-degree = 1.
        if e == n - 1 {
            let max_in = self
                .graph
                .node_indices()
                .map(|i| {
                    self.graph
                        .neighbors_directed(i, Direction::Incoming)
                        .count()
                })
                .max()
                .unwrap_or(0);
            let max_out = self
                .graph
                .node_indices()
                .map(|i| {
                    self.graph
                        .neighbors_directed(i, Direction::Outgoing)
                        .count()
                })
                .max()
                .unwrap_or(0);

            if max_in <= 1 && max_out <= 1 {
                return DagShape::LinearChain;
            }
            return DagShape::Tree;
        }

        // Wide fan-out: many roots (no incoming edges), shallow depth.
        let tiers = self.compute_tiers();
        let max_tier = tiers.values().copied().max().unwrap_or(0);
        let roots = self
            .graph
            .node_indices()
            .filter(|&i| {
                self.graph
                    .neighbors_directed(i, Direction::Incoming)
                    .count()
                    == 0
            })
            .count();

        if max_tier <= 2 && roots > n / 2 {
            return DagShape::WideFanOut;
        }

        // Diamond: has at least one node with in-degree > 1 (fan-in).
        let has_fanin = self.graph.node_indices().any(|i| {
            self.graph
                .neighbors_directed(i, Direction::Incoming)
                .count()
                > 1
        });
        let has_fanout = self.graph.node_indices().any(|i| {
            self.graph
                .neighbors_directed(i, Direction::Outgoing)
                .count()
                > 1
        });

        if has_fanin && has_fanout {
            return DagShape::Diamond;
        }

        DagShape::Complex
    }

    /// Compute plan statistics.
    pub fn stats(&self) -> PlanStats {
        let tiers = self.compute_tiers();
        let max_tier = tiers.values().copied().max().map(|t| t + 1).unwrap_or(0);

        // Count steps per tier for max parallelism.
        let mut tier_counts: HashMap<usize, usize> = HashMap::new();
        for tier in tiers.values() {
            *tier_counts.entry(*tier).or_default() += 1;
        }
        let max_parallelism = tier_counts.values().copied().max().unwrap_or(0);

        // Count by kind.
        let mut assets = 0;
        let mut sensors = 0;
        let mut effects = 0;
        for idx in self.graph.node_indices() {
            match self.graph[idx].kind {
                NodeKind::Asset => assets += 1,
                NodeKind::Sensor => sensors += 1,
                NodeKind::Effect => effects += 1,
            }
        }

        PlanStats {
            total_steps: self.graph.node_count(),
            skippable_steps: 0, // determined at runtime via cache lookup
            max_parallelism,
            critical_path_length: max_tier,
            by_kind: KindBreakdown {
                assets,
                sensors,
                effects,
            },
        }
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

    /// Node count.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Edge count.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}
