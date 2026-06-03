//! Execution plan generation — transforms a DAG into an ordered, tiered plan
//! optimized for parallel execution.
//!
//! Different DAG shapes produce different plan characteristics:
//! - Linear chain: N tiers, 1 step per tier (no parallelism)
//! - Wide fan-out: few tiers, many steps per tier (max parallelism)
//! - Diamond: mix of serial and parallel phases
//! - Partition expansion: single logical step → N parallel partition executions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::model::{EdgeKind, Freshness, NodeKind, PartitionSpec, SerializerKind};

/// A fully resolved execution plan — ready to be sent to a Python worker pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Unique plan ID (for DB persistence and worker dispatch).
    pub plan_id: String,
    /// Steps in topological order.
    pub steps: Vec<ExecutionStep>,
    /// Total number of parallelism tiers.
    pub total_tiers: usize,
    /// Plan-level statistics.
    pub stats: PlanStats,
    /// Resource constraints — controls runtime scheduling.
    pub resource_config: ResourceConfig,
}

/// Resource configuration — the knobs the user controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Max concurrent steps (default: cpu_count). The primary user-facing knob.
    /// Set to 1 for fully sequential execution (e.g., when each step uses all cores).
    pub pool_size: usize,
    /// Concurrency group limits. Key = group name, value = max concurrent steps in that group.
    /// Derived from `tags={"concurrency_group": "..."}` on nodes.
    /// Example: {"gpu": 1, "network": 8}
    pub concurrency_groups: HashMap<String, usize>,
}

/// An individual execution step — one function call to dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    /// Unique step ID within this plan.
    pub step_id: String,
    /// The node this step materializes.
    pub node_id: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Function to call.
    pub function_name: String,
    /// Source file containing the function.
    pub source_file: String,
    /// Resolved inputs: param_name → upstream_step_id (or node_id for cached).
    pub inputs: HashMap<String, InputSource>,
    /// Partition key for this step (if partitioned). None for non-partitioned.
    pub partition: Option<PartitionKey>,
    /// Parallelism tier — steps in the same tier can execute concurrently.
    pub tier: usize,
    /// Timeout for this step.
    pub timeout_seconds: u32,
    /// Sinks to write after successful execution.
    pub sinks: Vec<SinkStep>,
    /// Execution strategy hint.
    pub strategy: StepStrategy,
}

/// Where a step gets its input from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputSource {
    /// From another step in this plan (in-memory pass-through).
    Step { step_id: String, param_name: String },
    /// From a cached materialization (load from DB/disk).
    Cached {
        node_id: String,
        materialization_id: Option<String>,
    },
    /// Collected from multiple partition steps.
    Collected { step_ids: Vec<String> },
}

/// A partition key — the specific partition this step runs for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionKey {
    /// Dimension → value mapping.
    pub dimensions: HashMap<String, String>,
}

impl PartitionKey {
    pub fn single(dim: &str, value: &str) -> Self {
        let mut dimensions = HashMap::new();
        dimensions.insert(dim.to_string(), value.to_string());
        Self { dimensions }
    }
}

/// A sink write to perform after step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkStep {
    pub path: String,
    pub serializer: Option<SerializerKind>,
}

/// Execution strategy hint — tells the worker pool how to handle this step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStrategy {
    /// Normal execution — call the function, cache the result.
    Normal,
    /// Sensor poll — call function, interpret (bool, output) tuple.
    SensorPoll,
    /// Effect — call function, don't cache, fire-and-forget semantics.
    Effect,
    /// Skip — this step was determined to be fresh (cache hit).
    Skip,
}

/// Statistics about the generated plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStats {
    /// Total steps (including partition expansions).
    pub total_steps: usize,
    /// Steps that can be skipped (cache hits).
    pub skippable_steps: usize,
    /// Maximum width (most steps in a single tier).
    pub max_parallelism: usize,
    /// Critical path length (longest sequential chain).
    pub critical_path_length: usize,
    /// Breakdown by node kind.
    pub by_kind: KindBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindBreakdown {
    pub assets: usize,
    pub sensors: usize,
    pub effects: usize,
}

// ─── Plan optimization hints ─────────────────────────────────────────────────

/// DAG shape classification — informs optimization strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DagShape {
    /// Pure linear chain (e.g., ETL pipeline). No parallelism possible.
    LinearChain,
    /// Wide and shallow — many independent nodes (e.g., 500 independent assets).
    WideFanOut,
    /// Diamond — fan-out then fan-in (e.g., spaceflights: sources → merge → train).
    Diamond,
    /// Linear chain where each node is partitioned — N partitions × M stages.
    /// Parallelism across partitions, sequential within each partition's chain.
    /// Can be pipelined: start stage 2 for partition A as soon as stage 1 for A completes.
    PartitionedChain,
    /// Deep tree with branches that never reconverge.
    Tree,
    /// Mixed / complex topology.
    Complex,
}

/// Execution mode for partitioned chains — controls scheduling order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// "No man left behind" — complete ALL partitions at stage N before advancing
    /// any partition to stage N+1. Partitions advance together as a cohort.
    ///
    /// Good when: downstream uses `collect()` (needs all partitions), you want
    /// consistent stage-level snapshots, or stages have shared setup cost.
    ///
    /// Parallelism: all partitions within each stage run concurrently.
    Breadth,

    /// "First past the finish" — complete an entire chain for one partition before
    /// starting the next. Each partition runs its full pipeline independently.
    ///
    /// Good when: you want earliest possible output, memory is constrained
    /// (don't hold N intermediate results), or partition results are independent.
    ///
    /// Parallelism: multiple full chains run concurrently (up to worker pool size).
    Depth,

    /// Auto-detect based on DAG structure:
    /// - If downstream has `collect()` → must use Breadth (needs all partitions).
    /// - If no fan-in after chain → use Depth (better latency).
    Auto,
}

/// Plan-level optimization decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOptimizations {
    /// Classified DAG shape.
    pub shape: DagShape,
    /// Execution mode for partitioned regions of the plan.
    pub execution_mode: ExecutionMode,
    /// Recommended worker pool size for this plan.
    pub recommended_workers: usize,
    /// Whether in-memory result passing is feasible (vs. going through DB).
    /// True when plan fits in a single process's memory.
    pub memory_passthrough: bool,
    /// Steps that form the critical path (longest sequential dependency chain).
    pub critical_path: Vec<String>,
    /// Partition batching: group partition steps for bulk dispatch.
    pub partition_batches: Vec<PartitionBatch>,
}

/// A batch of partition steps that can be dispatched together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionBatch {
    /// The logical node being partitioned.
    pub node_id: String,
    /// Step IDs in this batch.
    pub step_ids: Vec<String>,
    /// Recommended batch size for the worker pool.
    pub batch_size: usize,
}
