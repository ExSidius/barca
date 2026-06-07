pub mod cache;
pub mod commands;
pub mod cone;
pub mod dag;
pub mod db;
pub mod dispatch;
pub mod executor;
pub mod hash;
pub mod model;
pub mod parse;
pub mod planner;
pub mod protocol;
pub mod scheduler;
pub mod work_plan;

pub use dag::Dag;
pub use model::*;
pub use planner::{ExecutionPlan, ResourceConfig, expand_partition_combos};

/// Top-level error type for barca engine operations.
#[derive(Debug, thiserror::Error)]
pub enum BarcaError {
    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("Asset '{0}' not found. Available: {1}")]
    AssetNotFound(String, String),

    #[error("DAG error: {0}")]
    Dag(#[from] dag::DagError),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Worker failed: {0}")]
    WorkerFailed(String),

    #[error("Database error: {0}")]
    Db(String),

    #[error("{0}")]
    Other(String),
}
