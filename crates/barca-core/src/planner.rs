//! Execution planner — transforms a DAG into a phased, multi-stream execution plan.
//!
//! Flow: Dag → decompose() → Topology → plan() → ExecutionPlan
//!
//! 1. decompose: identify chains, fan-out points, fan-in points
//! 2. plan: assign chains to phases and streams based on topology + resource config

use std::collections::{HashMap, HashSet, VecDeque};

use crate::dag::Dag;
use crate::plan::ResourceConfig;

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

    // ── Decomposition tests ──────────────────────────────────────────────────

    #[test]
    fn decompose_linear_chain() {
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
        let dag = build_test_dag(&[("a", &[]), ("b", &[]), ("c", &[])]);
        let topo = decompose(&dag);
        // 3 separate chains of length 1
        assert_eq!(topo.chains.len(), 3);
        for chain in &topo.chains {
            assert_eq!(chain.nodes.len(), 1);
        }
    }

    #[test]
    fn decompose_diamond() {
        // a → [b, c] → d
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b", "c"])]);
        let topo = decompose(&dag);
        // a is alone (has 2 succs), b alone, c alone, d alone
        assert_eq!(topo.chains.len(), 4);
    }

    #[test]
    fn decompose_chain_then_fan_out() {
        // a → b → c → [d, e, f]
        let dag = build_test_dag(&[
            ("a", &[]),
            ("b", &["a"]),
            ("c", &["b"]),
            ("d", &["c"]),
            ("e", &["c"]),
            ("f", &["c"]),
        ]);
        let topo = decompose(&dag);
        // Chain [a, b] (c has 3 succs so breaks), then c alone, d, e, f alone
        // Actually: a→b stops because b's succ (c) has 1 pred... let me think
        // a has 1 succ (b), b has 1 pred (a) → extend. b has 1 succ (c), c has 1 pred (b) → extend.
        // c has 3 succs → stop.
        // So chain is [a, b, c], then d, e, f are separate.
        assert_eq!(topo.chains.len(), 4); // [a,b,c], [d], [e], [f]
        assert_eq!(topo.chains[0].nodes.len(), 3);
    }

    #[test]
    fn decompose_spaceflights() {
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
        // Chains: [raw_s→prep_s], [raw_c→prep_c], [raw_r→prep_r], [master→split] (train has 1 succ eval, but eval has 2 preds → break)
        // Actually: master has 1 succ (split), split has 1 pred (master) → extend.
        // split has 2 succs (train, eval) → stop. So chain is [master, split].
        // train has 1 succ (eval), eval has 2 preds → stop. Chain [train].
        // eval alone.
        // Total: [raw_s,prep_s], [raw_c,prep_c], [raw_r,prep_r], [master,split], [train], [eval]
        assert_eq!(topo.chains.len(), 6);
    }

    // ── Plan tests ───────────────────────────────────────────────────────────

    #[test]
    fn plan_single_node() {
        let dag = build_test_dag(&[("a", &[])]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 1);
        assert_eq!(p.total_steps, 1);
    }

    #[test]
    fn plan_linear_chain_one_stream() {
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.phases.len(), 1);
        assert_eq!(p.phases[0].streams.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 3);
        // Correct order
        let ids: Vec<&str> = p.phases[0].streams[0]
            .steps
            .iter()
            .map(|s| s.node_id.as_str())
            .collect();
        assert_eq!(ids, vec!["test.py:a", "test.py:b", "test.py:c"]);
    }

    #[test]
    fn plan_fan_out_distributes() {
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
        // Roughly balanced: 4, 3, 3 or similar
        for stream in &p.phases[0].streams {
            assert!(stream.steps.len() >= 3);
            assert!(stream.steps.len() <= 4);
        }
    }

    #[test]
    fn plan_diamond_has_three_phases() {
        let dag = build_test_dag(&[("a", &[]), ("b", &["a"]), ("c", &["a"]), ("d", &["b", "c"])]);
        let p = plan_from_dag(&dag, &cfg(10));
        assert_eq!(p.total_steps, 4);
        // a in phase 0, b+c in phase 1, d in phase 2
        assert_eq!(p.phases.len(), 3);
    }

    #[test]
    fn plan_chain_fan_out_fan_in() {
        // a → b → c → [d0..d9] → e
        let mut specs: Vec<(&str, Vec<&str>)> =
            vec![("a", vec![]), ("b", vec!["a"]), ("c", vec!["b"])];
        let d_names: Vec<String> = (0..10).map(|i| format!("d{i}")).collect();
        for name in &d_names {
            specs.push((name, vec!["c"]));
        }
        let d_refs: Vec<&str> = d_names.iter().map(|s| s.as_str()).collect();
        specs.push(("e", d_refs));

        // Convert to (&str, &[&str]) format
        let specs_ref: Vec<(&str, &[&str])> =
            specs.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        let dag = build_test_dag(&specs_ref);
        let p = plan_from_dag(&dag, &cfg(5));

        // Phase 0: chain [a, b, c] → 1 stream
        assert_eq!(p.phases[0].streams.len(), 1);
        assert_eq!(p.phases[0].streams[0].steps.len(), 3);

        // Phase 1: 10 d-nodes → distributed across up to 5 streams
        let phase1_steps: usize = p.phases[1].streams.iter().map(|s| s.steps.len()).sum();
        assert_eq!(phase1_steps, 10);
        assert!(p.phases[1].streams.len() <= 5);

        // Phase 2: e → 1 stream
        let last_phase = p.phases.last().unwrap();
        assert_eq!(
            last_phase
                .streams
                .iter()
                .map(|s| s.steps.len())
                .sum::<usize>(),
            1
        );
    }

    #[test]
    fn plan_spaceflights() {
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

        // Phase 0: 3 chains [raw_s→prep_s], [raw_c→prep_c], [raw_r→prep_r] → 3 streams
        assert_eq!(p.phases[0].streams.len(), 3);
        for stream in &p.phases[0].streams {
            assert_eq!(stream.steps.len(), 2);
        }

        // Total = 10
        assert_eq!(p.total_steps, 10);
    }

    #[test]
    fn plan_pool_size_one_single_stream() {
        let dag = build_test_dag(&[("a", &[]), ("b", &[]), ("c", &[])]);
        let p = plan_from_dag(&dag, &cfg(1));
        for phase in &p.phases {
            assert_eq!(phase.streams.len(), 1);
        }
        assert_eq!(p.total_steps, 3);
    }
}
