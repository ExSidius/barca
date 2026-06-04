//! Barca CLI — invisible asset orchestrator.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "barca", about = "Invisible asset orchestrator", version)]
enum Cli {
    /// Parse, plan, and execute all assets in one or more files
    Run {
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Get a fresh asset value (cache-aware, only runs the target's subgraph)
    Get {
        /// Target asset function name
        target: String,
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Parse source files and emit the execution plan as JSON
    Plan {
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    let python = barca_core::commands::find_python();

    match cli {
        Cli::Run { files } => {
            let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            let result = barca_core::commands::run(&file_args, &python);
            let final_output = result
                .final_output
                .as_ref()
                .map(|oref| read_final_output(oref));
            println!(
                "{}",
                serde_json::json!({
                    "elapsed_seconds": result.elapsed_seconds,
                    "steps_executed": result.steps_executed,
                    "phases": result.phases,
                    "final_output": final_output,
                })
            );
        }
        Cli::Get { target, files } => {
            let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            let result = barca_core::commands::get(&target, &file_args, &python);
            let final_output = result
                .final_output
                .as_ref()
                .map(|oref| read_final_output(oref));
            println!(
                "{}",
                serde_json::json!({
                    "elapsed_seconds": result.elapsed_seconds,
                    "steps_executed": result.steps_executed,
                    "final_output": final_output,
                })
            );
        }
        Cli::Plan { files } => {
            let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            let result = barca_core::commands::plan(&file_args, &python);
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
    }
}

/// Read an artifact for display: inline JSON values, show metadata for binary formats.
fn read_final_output(oref: &barca_core::dispatch::OutputRef) -> serde_json::Value {
    if oref.format == "json" {
        std::fs::read_to_string(&oref.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| artifact_metadata(oref))
    } else {
        artifact_metadata(oref)
    }
}

fn artifact_metadata(oref: &barca_core::dispatch::OutputRef) -> serde_json::Value {
    serde_json::json!({
        "artifact_path": oref.path,
        "artifact_format": oref.format,
        "artifact_size_bytes": oref.size_bytes,
    })
}
