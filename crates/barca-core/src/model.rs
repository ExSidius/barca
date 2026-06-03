//! Core domain model — every concept from the barca workflow specification.
//!
//! These types represent the *declared* state of a barca project as parsed
//! from Python source files. They are the input to DAG construction, execution
//! planning, and hashing.
//!
//! Allocation strategy:
//! - `SmallVec<[T; N]>` for small collections (inputs, sinks — usually ≤4 items, stays on stack)
//! - `String` everywhere else (ruff's AST already gives us owned Strings; no point converting)

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;

// ─── Node kinds ──────────────────────────────────────────────────────────────

/// The three kinds of node in a barca DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// `@asset` — produces and caches a value.
    Asset,
    /// `@sensor` — observes external state, returns `(updated: bool, output)`.
    Sensor,
    /// `@effect` — side-effect leaf node, cannot be an input to other nodes.
    Effect,
}

// ─── Freshness ───────────────────────────────────────────────────────────────

/// Freshness policy — determines when a node is eligible for execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Freshness {
    /// Auto-materializes whenever stale and all upstreams are fresh.
    Always,
    /// Only runs on explicit `barca assets refresh`.
    Manual,
    /// Runs when a cron tick has elapsed since last execution.
    Schedule(CronExpr),
}

impl Freshness {
    /// Default freshness for a given node kind.
    pub fn default_for(kind: NodeKind) -> Self {
        match kind {
            NodeKind::Asset | NodeKind::Effect => Freshness::Always,
            NodeKind::Sensor => Freshness::Manual,
        }
    }
}

/// A validated 5-field cron expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CronExpr(pub String);

// ─── Partition specification ─────────────────────────────────────────────────

/// How partitions are defined for an asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PartitionSpec {
    /// Inline static list: `partitions(["AAPL", "MSFT", "GOOG"])`.
    Static { values: Vec<PartitionValue> },
    /// Derived from upstream asset: `partitions_from(tickers)`.
    DerivedFrom { source_ref: NodeRef },
}

/// A single partition value — scalar types allowed in partition keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PartitionValue {
    Str(String),
    Int(i64),
}

// ─── Input references ────────────────────────────────────────────────────────

/// A declared input to a node — maps a function parameter to an upstream node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredInput {
    /// The parameter name in the function signature.
    pub param_name: String,
    /// Reference to the upstream node.
    pub upstream: NodeRef,
    /// Whether this input uses `collect()` (fan-in from all partitions).
    pub collected: bool,
}

/// A reference to another node — either a function name (local) or a canonical path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum NodeRef {
    /// Direct function reference: just the function name (resolved during DAG build).
    FunctionName(String),
    /// Canonical asset reference: `asset_ref("module/file.py:function_name")`.
    Canonical(String),
}

impl NodeRef {
    /// The name used for resolution (function name or the name part of canonical).
    pub fn resolution_name(&self) -> &str {
        match self {
            NodeRef::FunctionName(name) => name,
            NodeRef::Canonical(path) => path.rsplit(':').next().unwrap_or(path),
        }
    }
}

// ─── Sink declaration ────────────────────────────────────────────────────────

/// A `@sink` decorator stacked on an `@asset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SinkDecl {
    /// Output path (local, s3://, gs://, etc.).
    pub path: String,
    /// Serializer kind override.
    pub serializer: Option<SerializerKind>,
}

/// Supported serializer types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SerializerKind {
    Json,
    Parquet,
    Pickle,
    Text,
    Yaml,
}

// ─── Extracted node ──────────────────────────────────────────────────────────

/// A fully extracted node from Python source — the output of parsing.
/// Contains all declared metadata needed for DAG construction and planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedNode {
    /// The kind of node.
    pub kind: NodeKind,
    /// The Python function name.
    pub function_name: String,
    /// Explicit `name=` override for continuity key.
    pub explicit_name: Option<String>,
    /// Freshness policy.
    pub freshness: Freshness,
    /// Declared inputs (parameter → upstream mapping). Typically 0–4 items.
    pub inputs: SmallVec<[DeclaredInput; 4]>,
    /// Partition dimensions.
    pub partitions: HashMap<String, PartitionSpec>,
    /// Sink declarations (from stacked `@sink` decorators). Typically 0–2 items.
    pub sinks: SmallVec<[SinkDecl; 2]>,
    /// Timeout in seconds.
    pub timeout_seconds: u32,
    /// Human-readable description.
    pub description: Option<String>,
    /// Metadata tags.
    pub tags: HashMap<String, String>,
    /// Whether this function is marked `@unsafe`.
    pub is_unsafe: bool,
    /// Source file (repo-relative path).
    pub source_file: String,
    /// Byte offset of the function definition in source.
    pub byte_offset: usize,
    /// The raw source text of the function (for hashing).
    pub source_text: String,
}

impl ExtractedNode {
    /// The continuity key — stable identity for this node.
    pub fn continuity_key(&self) -> String {
        if let Some(ref name) = self.explicit_name {
            name.clone()
        } else {
            format!("{}:{}", self.source_file, self.function_name)
        }
    }
}

// ─── DAG node (enriched after construction) ──────────────────────────────────

/// A node in the constructed DAG — enriched with resolved information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    /// Stable identity.
    pub id: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Function name.
    pub function_name: String,
    /// Source file path.
    pub source_file: String,
    /// Freshness policy.
    pub freshness: Freshness,
    /// Resolved inputs: param_name → upstream_node_id.
    pub inputs: HashMap<String, String>,
    /// Collected inputs (fan-in): param_name → upstream_node_id.
    pub collected_inputs: HashMap<String, String>,
    /// Partition dimensions. Typically 0–2 keys.
    pub partition_keys: SmallVec<[String; 2]>,
    /// Whether partitions are static or derived.
    pub partition_specs: HashMap<String, PartitionSpec>,
    /// Sink declarations.
    pub sinks: SmallVec<[SinkDecl; 2]>,
    /// Timeout.
    pub timeout_seconds: u32,
    /// Tags.
    pub tags: HashMap<String, String>,
    /// Whether @unsafe.
    pub is_unsafe: bool,
}

// ─── Edge kinds ──────────────────────────────────────────────────────────────

/// How two nodes are related in the DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Direct 1:1 dependency (partition-aligned if both partitioned).
    Direct,
    /// Fan-in: downstream consumes all partitions of upstream via `collect()`.
    Collect,
    /// Partition source: upstream defines the partition universe for downstream.
    PartitionSource,
}
