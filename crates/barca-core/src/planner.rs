//! Execution planner — transforms a DAG into a phased, multi-stream execution plan.
//!
//! Flow: Dag → decompose() → Topology → plan() → ExecutionPlan
//!
//! 1. decompose: identify chains, fan-out points, fan-in points
//! 2. plan: assign chains to phases and streams based on topology + resource config

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::dag::Dag;

/// Resource configuration — the knobs the user controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Max concurrent workers (default: cpu_count).
    pub pool_size: usize,
    /// Per-group concurrency limits from `tags={"concurrency_group": "..."}`.
    pub concurrency_groups: HashMap<String, usize>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Output types
// ═══════════════════════════════════════════════════════════════════════════════

/// A complete execution plan — phased, with worker stream assignments.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub phases: Vec<Phase>,
    pub total_steps: usize,
}

/// A phase — all streams execute concurrently; all must complete before next phase.
#[derive(Debug, Clone)]
pub struct Phase {
    pub reason: PhaseReason,
    pub streams: Vec<WorkerStream>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseReason {
    Initial,
    FanIn { node_id: String },
    PartitionResolution { source_node_id: String },
}

/// A worker stream — ordered steps for one Python process.
#[derive(Debug, Clone)]
pub struct WorkerStream {
    pub stream_id: String,
    pub steps: Vec<StreamStep>,
}

/// A step within a stream.
#[derive(Debug, Clone)]
pub struct StreamStep {
    pub node_id: String,
    pub function_name: String,
    pub source_file: String,
    pub inputs: HashMap<String, String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Intermediate representation: Topology
// ═══════════════════════════════════════════════════════════════════════════════

/// The decomposed structure of a DAG — chains and their relationships.
/// This is the intermediate between raw DAG and execution plan.
#[derive(Debug, Clone)]
pub struct Topology {
    /// All chains (maximal sequential paths). Every node is in exactly one chain.
    pub chains: Vec<Chain>,
    /// Which chains each chain depends on.
    pub chain_deps: HashMap<usize, HashSet<usize>>,
    /// Node → chain index lookup.
    pub node_to_chain: HashMap<String, usize>,
}

/// A chain: maximal path where each internal node has single predecessor and successor.
#[derive(Debug, Clone)]
pub struct Chain {
    /// Ordered node IDs (head to tail).
    pub nodes: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Decompose DAG → Topology
// ═══════════════════════════════════════════════════════════════════════════════

/// Decompose a DAG into its topological structure (chains + relationships).
pub fn decompose(dag: &Dag) -> Topology {
    let chains = detect_chains(dag);

    // Build node → chain lookup.
    let mut node_to_chain: HashMap<String, usize> = HashMap::new();
    for (idx, chain) in chains.iter().enumerate() {
        for node_id in &chain.nodes {
            node_to_chain.insert(node_id.clone(), idx);
        }
    }

    // Compute inter-chain dependencies.
    let mut chain_deps: HashMap<usize, HashSet<usize>> = HashMap::new();
    for (idx, chain) in chains.iter().enumerate() {
        let mut deps = HashSet::new();
        for node_id in &chain.nodes {
            for upstream_id in dag.upstream(node_id) {
                let upstream_chain = node_to_chain[upstream_id];
                if upstream_chain != idx {
                    deps.insert(upstream_chain);
                }
            }
        }
        chain_deps.insert(idx, deps);
    }

    Topology {
        chains,
        chain_deps,
        node_to_chain,
    }
}

/// Detect all maximal chains. Every node belongs to exactly one chain.
fn detect_chains(dag: &Dag) -> Vec<Chain> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut chains = Vec::new();
    let topo = dag.topo_order();

    for node_id in topo {
        if visited.contains(node_id) {
            continue;
        }

        let mut nodes = vec![node_id.to_string()];
        visited.insert(node_id.to_string());

        // Extend forward: follow single-succ + single-pred edges.
        let mut current = node_id.to_string();
        loop {
            let succs = dag.downstream(&current);
            if succs.len() != 1 {
                break;
            }
            let next = succs[0];
            if dag.upstream(next).len() != 1 {
                break;
            }
            if visited.contains(next) {
                break;
            }
            nodes.push(next.to_string());
            visited.insert(next.to_string());
            current = next.to_string();
        }

        chains.push(Chain { nodes });
    }

    chains
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Plan — Topology × ResourceConfig → ExecutionPlan
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate an execution plan from a topology and resource config.
pub fn plan(dag: &Dag, topology: &Topology, config: &ResourceConfig) -> ExecutionPlan {
    let phase_assignment = assign_phases(topology);
    let phases = build_phases(dag, topology, &phase_assignment, config);
    let phases = merge_single_stream_phases(phases);

    let total_steps = phases
        .iter()
        .flat_map(|p| &p.streams)
        .map(|s| s.steps.len())
        .sum();

    ExecutionPlan {
        phases,
        total_steps,
    }
}

/// Merge consecutive phases that each have exactly 1 stream into a single phase.
/// This avoids spawning a new process for each sequential phase when there's
/// no parallelism opportunity.
fn merge_single_stream_phases(phases: Vec<Phase>) -> Vec<Phase> {
    let mut merged: Vec<Phase> = Vec::new();

    for phase in phases {
        let can_merge = phase.streams.len() == 1
            && merged
                .last()
                .is_some_and(|prev: &Phase| prev.streams.len() == 1);

        if can_merge {
            // Append this phase's single stream's steps to the previous phase's single stream.
            let prev = merged.last_mut().unwrap();
            let new_steps = phase.streams.into_iter().next().unwrap().steps;
            prev.streams[0].steps.extend(new_steps);
        } else {
            merged.push(phase);
        }
    }

    merged
}

/// Assign each chain to a phase number based on inter-chain dependencies.
/// chain_idx → phase_number
fn assign_phases(topology: &Topology) -> HashMap<usize, usize> {
    let chain_order = topo_sort_indices(topology.chains.len(), &topology.chain_deps);
    let mut chain_phase: HashMap<usize, usize> = HashMap::new();

    for &chain_idx in &chain_order {
        let deps = &topology.chain_deps[&chain_idx];
        let phase = if deps.is_empty() {
            0
        } else {
            deps.iter()
                .map(|d| chain_phase.get(d).copied().unwrap_or(0) + 1)
                .max()
                .unwrap_or(0)
        };
        chain_phase.insert(chain_idx, phase);
    }

    chain_phase
}

/// Build Phase structs with stream assignments.
fn build_phases(
    dag: &Dag,
    topology: &Topology,
    phase_assignment: &HashMap<usize, usize>,
    config: &ResourceConfig,
) -> Vec<Phase> {
    let max_phase = phase_assignment.values().copied().max().unwrap_or(0);

    let mut phases = Vec::new();
    for phase_num in 0..=max_phase {
        // Chains in this phase.
        let mut chains_in_phase: Vec<usize> = phase_assignment
            .iter()
            .filter(|&(_, &p)| p == phase_num)
            .map(|(&c, _)| c)
            .collect();
        chains_in_phase.sort(); // determinism

        let reason = if phase_num == 0 {
            PhaseReason::Initial
        } else {
            let first_chain = chains_in_phase.first().copied().unwrap_or(0);
            PhaseReason::FanIn {
                node_id: topology.chains[first_chain].nodes[0].clone(),
            }
        };

        // Convert chains to step lists.
        let chain_steps: Vec<Vec<StreamStep>> = chains_in_phase
            .iter()
            .map(|&chain_idx| chain_to_steps(dag, &topology.chains[chain_idx]))
            .collect();

        // Assign to streams via greedy bin-packing.
        let num_streams = chains_in_phase.len().min(config.pool_size).max(1);
        let streams = bin_pack_to_streams(chain_steps, num_streams, phase_num);

        phases.push(Phase { reason, streams });
    }

    phases
}

/// Convert a chain's nodes into StreamSteps.
fn chain_to_steps(dag: &Dag, chain: &Chain) -> Vec<StreamStep> {
    chain
        .nodes
        .iter()
        .map(|node_id| {
            let node = dag.get_node(node_id).unwrap();
            let mut inputs = node.inputs.clone();
            for (k, v) in &node.collected_inputs {
                inputs.insert(k.clone(), v.clone());
            }
            StreamStep {
                node_id: node_id.clone(),
                function_name: node.function_name.clone(),
                source_file: node.source_file.clone(),
                inputs,
            }
        })
        .collect()
}

/// Distribute chains across N streams using greedy bin-packing (longest-first).
fn bin_pack_to_streams(
    mut chain_steps: Vec<Vec<StreamStep>>,
    num_streams: usize,
    phase_idx: usize,
) -> Vec<WorkerStream> {
    // Sort chains by length, longest first.
    chain_steps.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let mut streams: Vec<Vec<StreamStep>> = vec![Vec::new(); num_streams];
    let mut stream_sizes: Vec<usize> = vec![0; num_streams];

    for steps in chain_steps {
        // Assign to stream with fewest steps.
        let target = stream_sizes
            .iter()
            .enumerate()
            .min_by_key(|&(_, &size)| size)
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        stream_sizes[target] += steps.len();
        streams[target].extend(steps);
    }

    streams
        .into_iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(idx, steps)| WorkerStream {
            stream_id: format!("p{phase_idx}-w{idx}"),
            steps,
        })
        .collect()
}

/// Topological sort of indices by dependency map.
fn topo_sort_indices(count: usize, deps: &HashMap<usize, HashSet<usize>>) -> Vec<usize> {
    let mut in_degree = vec![0usize; count];
    let mut successors = vec![Vec::new(); count];

    for (&node, node_deps) in deps {
        in_degree[node] = node_deps.len();
        for &dep in node_deps {
            successors[dep].push(node);
        }
    }

    let mut queue: VecDeque<usize> = (0..count).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(count);

    while let Some(node) = queue.pop_front() {
        order.push(node);
        for &succ in &successors[node] {
            in_degree[succ] -= 1;
            if in_degree[succ] == 0 {
                queue.push_back(succ);
            }
        }
    }

    order
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convenience: full pipeline
// ═══════════════════════════════════════════════════════════════════════════════

/// Full pipeline: DAG → decompose → plan.
pub fn plan_from_dag(dag: &Dag, config: &ResourceConfig) -> ExecutionPlan {
    let topology = decompose(dag);
    plan(dag, &topology, config)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use smallvec::SmallVec;

    fn build_test_dag(specs: &[(&str, &[&str])]) -> Dag {
        let extracted: Vec<ExtractedNode> = specs
            .iter()
            .map(|(name, deps)| {
                let inputs: SmallVec<[DeclaredInput; 4]> = deps
                    .iter()
                    .map(|&dep| DeclaredInput {
                        param_name: dep.to_string(),
                        upstream: NodeRef::FunctionName(dep.to_string()),
                        collected: false,
                    })
                    .collect();
                ExtractedNode {
                    kind: NodeKind::Asset,
                    function_name: name.to_string(),
                    explicit_name: None,
                    freshness: Freshness::Always,
                    inputs,
                    partitions: HashMap::new(),
                    sinks: SmallVec::new(),
                    timeout_seconds: 300,
                    description: None,
                    tags: HashMap::new(),
                    is_unsafe: false,
                    source_file: "test.py".to_string(),
                    byte_offset: 0,
                    source_text: String::new(),
                }
            })
            .collect();
        Dag::build(&extracted).unwrap()
    }

    fn cfg(pool_size: usize) -> ResourceConfig {
        ResourceConfig {
            pool_size,
            concurrency_groups: HashMap::new(),
        }
    }

    /// Helper: get step node_ids from a stream.
    fn step_ids(stream: &WorkerStream) -> Vec<&str> {
        stream.steps.iter().map(|s| s.node_id.as_str()).collect()
    }

    /// Helper: total steps across all streams in a phase.
    fn phase_step_count(phase: &Phase) -> usize {
        phase.streams.iter().map(|s| s.steps.len()).sum()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DECOMPOSITION: DAG shape → chains
    //
    // A chain is a maximal sequence where each node has single pred + succ.
    // Chains are the unit of vertical bundling (one Python process).
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn decompose_single_node() {
        // DAG: [a]
        // Chains: [a]
        let dag = build_test_dag(&[("a", &[])]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 1);
        assert_eq!(topo.chains[0].nodes, vec!["test.py:a"]);
    }

    #[test]
    fn decompose_linear_chain() {
        // DAG: a → b → c
        // Chains: [a, b, c] — one maximal chain
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 1);
        assert_eq!(
            topo.chains[0].nodes,
            vec!["test.py:a", "test.py:b", "test.py:c"]
        );
    }

    #[test]
    fn decompose_independent_nodes() {
        // DAG: a   b   c   (no edges)
        // Chains: [a], [b], [c] — each is its own chain
        let dag = build_test_dag(&[("a", &[]), ("b", &[]), ("c", &[])]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 3);
        for chain in &topo.chains {
            assert_eq!(chain.nodes.len(), 1);
        }
    }

    #[test]
    fn decompose_fan_out() {
        // DAG: a → [b, c, d]
        // a has 3 successors → chain breaks
        // Chains: [a], [b], [c], [d]
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["a"])]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 4);
    }

    #[test]
    fn decompose_fan_in() {
        // DAG: [a, b, c] → d
        // d has 3 predecessors → can't be chained with any
        // Chains: [a], [b], [c], [d]
        let dag = build_test_dag(&[("a", &[]), ("b", &[]), ("c", &[]), ("d", &["a", "b", "c"])]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 4);
    }

    #[test]
    fn decompose_diamond() {
        // DAG: a → [b, c] → d
        // a has 2 succs → break. d has 2 preds → break.
        // Chains: [a], [b], [c], [d]
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b", "c"])]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 4);
    }

    #[test]
    fn decompose_chain_then_fan_out() {
        // DAG: a → b → c → [d, e, f]
        // a→b→c is a chain (each has 1 pred, 1 succ until c which has 3 succs)
        // Chains: [a, b, c], [d], [e], [f]
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &["b"]),
            ("d", &["c"]),
            ("e", &["c"]),
            ("f", &["c"]),
        ]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 4);
        assert_eq!(topo.chains[0].nodes.len(), 3); // [a, b, c]
    }

    #[test]
    fn decompose_parallel_chains() {
        // DAG: a→b   c→d   e→f   (3 independent chains)
        // Chains: [a, b], [c, d], [e, f]
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &[]),
            ("d", &["c"]),
            ("e", &[]),
            ("f", &["e"]),
        ]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 3);
        for chain in &topo.chains {
            assert_eq!(chain.nodes.len(), 2);
        }
    }

    #[test]
    fn decompose_spaceflights() {
        // DAG:
        //   raw_s→prep_s ──┐
        //   raw_c→prep_c ──├→ master→split → train
        //   raw_r→prep_r ──┘              ↘ eval ← train
        //
        // Chains: [raw_s,prep_s], [raw_c,prep_c], [raw_r,prep_r],
        //         [master,split], [train], [eval]
        // (split has 2 succs → breaks; eval has 2 preds → breaks)
        let dag = build_test_dag(&[
            ("raw_s", &[]),
            ("raw_c", &[]),
            ("raw_r", &[]),
            ("prep_s", &["raw_s"]),
            ("prep_c", &["raw_c"]),
            ("prep_r", &["raw_r"]),
            ("master", &["prep_s", "prep_c", "prep_r"]),
            ("split", &["master"]),
            ("train", &["split"]),
            ("eval", &["train", "split"]),
        ]);
        let topo = decompose(&dag);
        assert_eq!(topo.chains.len(), 6);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PLANNING: Topology × ResourceConfig → ExecutionPlan
    //
    // The planner assigns chains to phases (barriers at fan-in points)
    // and streams (one Python process per stream).
    //
    // Rules:
    // - Chains with no cross-chain deps → same phase (parallel)
    // - Chain depends on chains from multiple phases → new phase (barrier)
    // - Consecutive single-stream phases merge (avoid redundant process spawn)
    // - Chains distributed across pool_size streams via bin-packing
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn plan_single_node() {
        // a → 1 phase, 1 stream, 1 step
        let dag = build_test_dag(&[("a", &[])]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 1);
    }

    #[test]
    fn plan_linear_chain_is_one_phase_one_stream() {
        // a → b → c → d → e
        // All in one chain → 1 phase, 1 stream, 5 steps in order
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &["b"]),
            ("d", &["c"]),
            ("e", &["d"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 5);
        let ids = step_ids(&p.phases[0].streams[0]);
        assert_eq!(
            ids,
            vec![
                "test.py:a",
                "test.py:b",
                "test.py:c",
                "test.py:d",
                "test.py:e"
            ]
        );
    }

    #[test]
    fn plan_independent_nodes_fan_out_to_streams() {
        // 10 independent nodes, pool_size=3
        // → 1 phase, 3 streams, ~3-4 steps each
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &[]),
            ("c", &[]),
            ("d", &[]),
            ("e", &[]),
            ("f", &[]),
            ("g", &[]),
            ("h", &[]),
            ("i", &[]),
            ("j", &[]),
        ]);
        let p = plan_from_dag(&dag, &cfg(3));
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 3);
        assert_eq!(p.total_steps, 10);
        for stream in &p.phases[0].streams {
            assert!(stream.steps.len() >= 3 && stream.steps.len() <= 4);
        }
    }

    #[test]
    fn plan_diamond_creates_phase_barrier() {
        // a → [b, c] → d
        //
        // Phase 0: [a] (1 stream)
        // Phase 1: [b], [c] (2 streams, parallel)
        // Phase 2: [d] (1 stream, barrier — needs both b and c)
        //
        // But phases 0 and 2 are single-stream → merge kicks in:
        // Phase 0: [a] merged with... wait, phase 1 has 2 streams so no merge.
        // Final: 3 phases.
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b", "c"])]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.phases.len(), 3);
        assert_eq!(phase_step_count(&p.phases[0]), 1); // a
        assert_eq!(phase_step_count(&p.phases[1]), 2); // b, c
        assert_eq!(phase_step_count(&p.phases[2]), 1); // d
    }

    #[test]
    fn plan_chain_then_fan_out_then_fan_in() {
        // a → b → c → [d0..d9] → e
        //
        // Decomposition: chain [a,b,c], then 10 single-node chains [d0]..[d9], then [e]
        //
        // Phase 0: 1 stream [a, b, c]
        // Phase 1: up to 5 streams with 10 d-nodes distributed
        // Phase 2: 1 stream [e]
        //
        // Phase 2 is single-stream but phase 1 has multiple → no merge.
        let mut specs: Vec<(&str, Vec<&str>)> =
            vec![("a", vec![]), ("b", vec!["a"]), ("c", vec!["b"])];
        let d_names: Vec<String> = (0..10).map(|i| format!("d{i}")).collect();
        for name in &d_names {
            specs.push((name, vec!["c"]));
        }
        let d_refs: Vec<&str> = d_names.iter().map(|s| s.as_str()).collect();
        specs.push(("e", d_refs));
        let specs_ref: Vec<(&str, &[&str])> =
            specs.iter().map(|(n, d)| (*n, d.as_slice())).collect();

        let dag = build_test_dag(&specs_ref);
        let p = plan_from_dag(&dag, &cfg(5));

        assert_eq!(p.phases[0].streams.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 3); // [a, b, c]
        assert_eq!(phase_step_count(&p.phases[1]), 10); // d0..d9
        assert!(p.phases[1].streams.len() <= 5); // distributed
        assert_eq!(phase_step_count(p.phases.last().unwrap()), 1); // [e]
    }

    #[test]
    fn plan_parallel_chains_in_one_phase() {
        // a→b   c→d   e→f   (3 independent 2-step chains)
        //
        // All chains have no cross-deps → 1 phase, 3 streams
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &[]),
            ("d", &["c"]),
            ("e", &[]),
            ("f", &["e"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 3);
        for stream in &p.phases[0].streams {
            assert_eq!(stream.steps.len(), 2);
        }
    }

    #[test]
    fn plan_spaceflights_diamond() {
        // raw_s→prep_s ──┐
        // raw_c→prep_c ──├→ master→split→train
        // raw_r→prep_r ──┘              ↘ eval
        //
        // Phase 0: 3 streams [raw_s,prep_s], [raw_c,prep_c], [raw_r,prep_r]
        // Phase 1: 1 stream [master,split,train,eval] (merged single-stream phases)
        let dag = build_test_dag(&[
            ("raw_s", &[]),
            ("raw_c", &[]),
            ("raw_r", &[]),
            ("prep_s", &["raw_s"]),
            ("prep_c", &["raw_c"]),
            ("prep_r", &["raw_r"]),
            ("master", &["prep_s", "prep_c", "prep_r"]),
            ("split", &["master"]),
            ("train", &["split"]),
            ("eval", &["train", "split"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(10));

        // Phase 0: 3 parallel source→prep chains
        assert_eq!(p.phases[0].streams.len(), 3);
        for stream in &p.phases[0].streams {
            assert_eq!(stream.steps.len(), 2);
        }
        // Remaining: master→split, train, eval = 4 steps (merged into 1 stream)
        let remaining: usize = p.phases[1..].iter().map(phase_step_count).sum();
        assert_eq!(remaining, 4);
        assert_eq!(p.total_steps, 10);
    }

    #[test]
    fn plan_wide_fan_in() {
        // [a, b, c, d, e, f, g, h, i, j] → merge
        //
        // Phase 0: 10 independent nodes → distributed across pool_size streams
        // Phase 1: [merge] → 1 stream (merged with nothing since phase 0 is multi-stream)
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &[]),
            ("c", &[]),
            ("d", &[]),
            ("e", &[]),
            ("f", &[]),
            ("g", &[]),
            ("h", &[]),
            ("i", &[]),
            ("j", &[]),
            ("merge", &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(5));
        assert_eq!(p.phases[0].streams.len(), 5); // 10 nodes across 5 streams
        assert_eq!(phase_step_count(&p.phases[0]), 10);
        assert_eq!(phase_step_count(p.phases.last().unwrap()), 1); // merge
        assert_eq!(p.total_steps, 11);
    }

    #[test]
    fn plan_consecutive_single_stream_phases_merge() {
        // a → b → c where b has 2 preds conceptually... actually let's
        // test directly: a → [b] → c (b is single chain, c depends only on b)
        // With just a→b→c this is one chain, one phase. So test with a diamond
        // that collapses to single-stream phases:
        //
        // a → b → c → d (all single-pred/single-succ)
        // This is one chain, so it's trivially 1 phase.
        //
        // Better test: the merge optimization.
        // Phase 0: [a] (1 stream), Phase 1: [b] (1 stream) → merge into 1 phase.
        // This happens when chains are in separate phases but both single-stream.
        //
        // Use spaceflights: phases after the fan-in (master,split,train,eval)
        // are all single-stream and should merge.
        let dag = build_test_dag(&[
            ("raw_s", &[]),
            ("prep_s", &["raw_s"]),
            ("master", &["prep_s"]),
            ("split", &["master"]),
            ("train", &["split"]),
            ("eval", &["train"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(10));
        // Everything is one chain [raw_s, prep_s, master, split, train, eval]
        // → 1 phase, 1 stream, 6 steps
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 6);
    }

    #[test]
    fn plan_pool_size_one_forces_sequential() {
        // 5 independent nodes, pool_size=1
        // → 1 phase, 1 stream containing all 5
        let dag = build_test_dag(&[("a", &[]), ("b", &[]), ("c", &[]), ("d", &[]), ("e", &[])]);
        let p = plan_from_dag(&dag, &cfg(1));
        for phase in &p.phases {
            assert_eq!(phase.streams.len(), 1);
        }
        assert_eq!(p.total_steps, 5);
    }

    #[test]
    fn plan_500_independent_across_16_workers() {
        // Simulates fan_out_500 benchmark
        let extracted: Vec<ExtractedNode> = (0..500)
            .map(|i| ExtractedNode {
                kind: NodeKind::Asset,
                function_name: format!("n{i:03}"),
                explicit_name: None,
                freshness: Freshness::Always,
                inputs: SmallVec::new(),
                partitions: HashMap::new(),
                sinks: SmallVec::new(),
                timeout_seconds: 300,
                description: None,
                tags: HashMap::new(),
                is_unsafe: false,
                source_file: "test.py".to_string(),
                byte_offset: 0,
                source_text: String::new(),
            })
            .collect();
        let dag = Dag::build(&extracted).unwrap();
        let p = plan_from_dag(&dag, &cfg(16));

        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 16);
        assert_eq!(p.total_steps, 500);
        // Each stream gets ~31-32 steps
        for stream in &p.phases[0].streams {
            assert!(stream.steps.len() >= 31 && stream.steps.len() <= 32);
        }
    }

    #[test]
    fn plan_wide_layers_with_sync_barriers() {
        // Layer 0: [a0, a1, a2] (independent)
        // Sync: merge0 depends on all of layer 0
        // Layer 1: [b0, b1, b2] depend on merge0
        // Sync: merge1 depends on all of layer 1
        //
        // Phase 0: [a0], [a1], [a2] → 3 streams
        // Phase 1: [merge0] → 1 stream (barrier)
        // Phase 2: [b0], [b1], [b2] → 3 streams
        // Phase 3: [merge1] → 1 stream (barrier)
        //
        // But phases 1+2 can't merge (1 is single-stream, 2 is multi-stream — no merge)
        // Phases 3 is single-stream, preceded by multi-stream — no merge.
        let dag = build_test_dag(&[
            ("a0", &[]),
            ("a1", &[]),
            ("a2", &[]),
            ("merge0", &["a0", "a1", "a2"]),
            ("b0", &["merge0"]),
            ("b1", &["merge0"]),
            ("b2", &["merge0"]),
            ("merge1", &["b0", "b1", "b2"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(10));

        assert_eq!(phase_step_count(&p.phases[0]), 3); // a0, a1, a2
        assert_eq!(p.total_steps, 8);
        // Should have barriers at merge points
        assert!(p.phases.len() >= 3); // at least: layer0, merge0+layer1, merge1
    }

    #[test]
    fn plan_deep_diamond_benchmark() {
        // 5 parallel 3-step chains → merge → 2-step tail
        // src_0→prep_0→feat_0 ──┐
        // src_1→prep_1→feat_1 ──├→ merge → transform → output
        // ...                   ┘
        //
        // Phase 0: 5 parallel streams, each [src, prep, feat]
        // Phase 1: [merge, transform, output] (merged single-stream)
        let dag = build_test_dag(&[
            ("s0", &[]),
            ("s1", &[]),
            ("s2", &[]),
            ("s3", &[]),
            ("s4", &[]),
            ("p0", &["s0"]),
            ("p1", &["s1"]),
            ("p2", &["s2"]),
            ("p3", &["s3"]),
            ("p4", &["s4"]),
            ("f0", &["p0"]),
            ("f1", &["p1"]),
            ("f2", &["p2"]),
            ("f3", &["p3"]),
            ("f4", &["p4"]),
            ("merge", &["f0", "f1", "f2", "f3", "f4"]),
            ("transform", &["merge"]),
            ("output", &["transform"]),
        ]);
        let p = plan_from_dag(&dag, &cfg(10));

        // Phase 0: 5 streams of 3 steps each
        assert_eq!(p.phases[0].streams.len(), 5);
        for stream in &p.phases[0].streams {
            assert_eq!(stream.steps.len(), 3);
        }
        // Remaining: merge + transform + output = 3 steps
        let remaining: usize = p.phases[1..].iter().map(phase_step_count).sum();
        assert_eq!(remaining, 3);
        assert_eq!(p.total_steps, 18);
    }
}
