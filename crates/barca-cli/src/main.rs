//! Barca CLI — invisible asset orchestrator.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum OutputMode {
    /// One-line JSON (default)
    #[default]
    Json,
    /// Just the final_output value, pretty-printed
    Value,
    /// Human-friendly with timing info
    Pretty,
}

#[derive(Parser)]
#[command(name = "barca", about = "Invisible asset orchestrator", version)]
enum Cli {
    /// Parse, plan, and execute all assets in one or more files
    Run {
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output format
        #[arg(short, long, default_value = "json")]
        output: OutputMode,
    },
    /// Get a fresh asset value (cache-aware, only runs the target's subgraph)
    Get {
        /// Target asset function name
        target: String,
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Output format
        #[arg(short, long, default_value = "json")]
        output: OutputMode,
    },
    /// Parse source files and emit the execution plan as JSON
    Plan {
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Print version information
    Version,
}

fn main() {
    // Support `barca file.py` as shorthand for `barca run file.py`.
    let cli = Cli::try_parse().unwrap_or_else(|_| {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 && !args[1].starts_with('-') && args[1].ends_with(".py") {
            Cli::Run {
                files: args[1..].iter().map(PathBuf::from).collect(),
                output: OutputMode::Json,
            }
        } else {
            Cli::parse() // re-parse to show proper clap error
        }
    });
    let python = barca_core::commands::find_python();

    let result = match cli {
        Cli::Run { files, output } => run_cmd(files, &python, output),
        Cli::Get {
            target,
            files,
            output,
        } => get_cmd(target, files, &python, output),
        Cli::Plan { files } => plan_cmd(files, &python),
        Cli::Version => {
            println!("barca {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run_cmd(
    files: Vec<PathBuf>,
    python: &PathBuf,
    mode: OutputMode,
) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::run(&file_args, python)?;
    let final_output = result
        .final_output
        .as_ref()
        .map(|oref| read_final_output(oref));

    match mode {
        OutputMode::Json => {
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
        OutputMode::Value => {
            if let Some(ref val) = final_output {
                println!("{}", serde_json::to_string_pretty(val).unwrap());
            }
        }
        OutputMode::Pretty => {
            println!(
                "Executed {} step(s) in {:.3}s ({} phase{})",
                result.steps_executed,
                result.elapsed_seconds,
                result.phases,
                if result.phases == 1 { "" } else { "s" }
            );
            if let Some(ref val) = final_output {
                println!("\nResult:\n{}", serde_json::to_string_pretty(val).unwrap());
            }
        }
    }
    Ok(())
}

fn get_cmd(
    target: String,
    files: Vec<PathBuf>,
    python: &PathBuf,
    mode: OutputMode,
) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::get(&target, &file_args, python)?;
    let final_output = result
        .final_output
        .as_ref()
        .map(|oref| read_final_output(oref));

    match mode {
        OutputMode::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "elapsed_seconds": result.elapsed_seconds,
                    "steps_executed": result.steps_executed,
                    "final_output": final_output,
                })
            );
        }
        OutputMode::Value => {
            if let Some(ref val) = final_output {
                println!("{}", serde_json::to_string_pretty(val).unwrap());
            }
        }
        OutputMode::Pretty => {
            println!(
                "Got '{}' in {:.3}s ({} step{})",
                target,
                result.elapsed_seconds,
                result.steps_executed,
                if result.steps_executed == 1 { "" } else { "s" }
            );
            if let Some(ref val) = final_output {
                println!("\nValue:\n{}", serde_json::to_string_pretty(val).unwrap());
            }
        }
    }
    Ok(())
}

fn plan_cmd(files: Vec<PathBuf>, python: &PathBuf) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::plan(&file_args, python)?;
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    Ok(())
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
        "_barca_artifact": {
            "path": oref.path,
            "format": oref.format,
            "size_bytes": oref.size_bytes,
        }
    })
}
