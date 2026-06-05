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
    /// Show recent run history
    History {
        /// Number of recent runs to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Show execution statistics for an asset
    Stats {
        /// Target asset function name
        target: String,
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Print version information
    Version,
}

fn main() {
    // Support `barca file.py [--flags]` as shorthand for `barca run file.py [--flags]`.
    let cli = Cli::try_parse().unwrap_or_else(|_| {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 && !args[1].starts_with('-') && args[1].ends_with(".py") {
            // Insert "run" after the program name so clap handles all flags.
            let mut rewritten = vec![args[0].clone(), "run".to_string()];
            rewritten.extend_from_slice(&args[1..]);
            Cli::parse_from(rewritten)
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
        Cli::History { limit } => history_cmd(limit),
        Cli::Stats { target, files } => stats_cmd(target, files, &python),
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
                    "run_id": result.run_id,
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
                "Run {} | {} step(s) in {:.3}s ({} phase{})",
                result.run_id,
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
                    "run_id": result.run_id,
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
                "Run {} | got '{}' in {:.3}s ({} step{})",
                result.run_id,
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

fn history_cmd(limit: usize) -> Result<(), barca_core::BarcaError> {
    let runs = barca_core::commands::history(limit)?;
    if runs.is_empty() {
        println!("No run history found.");
        return Ok(());
    }
    // Table header.
    println!(
        "{:<14} {:<7} {:<9} {:>5} {:>6} {:>6} {:<20}",
        "RUN_ID", "CMD", "STATUS", "STEPS", "CACHED", "TIME", "STARTED"
    );
    println!("{}", "-".repeat(75));
    for r in &runs {
        let elapsed_str = r
            .elapsed_seconds
            .map(|e| format!("{:.1}s", e))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<14} {:<7} {:<9} {:>5} {:>6} {:>6} {:<20}",
            r.run_id,
            r.command,
            r.status,
            r.steps_executed,
            r.steps_cached,
            elapsed_str,
            r.started_at,
        );
    }
    Ok(())
}

fn stats_cmd(
    target: String,
    files: Vec<PathBuf>,
    python: &PathBuf,
) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let stats = barca_core::commands::stats(&target, &file_args, python)?;
    println!("Asset: {}", stats.node_id);
    println!("Total materializations: {}", stats.total_runs);
    if let Some(avg) = stats.avg_elapsed_seconds {
        println!("Avg elapsed: {:.3}s", avg);
    } else {
        println!("Avg elapsed: -");
    }
    println!("Cache hit rate: {:.1}%", stats.cache_hit_rate * 100.0);
    if !stats.recent_runs.is_empty() {
        println!("\nRecent runs:");
        println!("  {:<10} {:<9} {:<20}", "ELAPSED", "STATUS", "CREATED");
        for entry in &stats.recent_runs {
            let elapsed_str = entry
                .elapsed_seconds
                .map(|e| format!("{:.3}s", e))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "  {:<10} {:<9} {:<20}",
                elapsed_str, entry.status, entry.created_at,
            );
        }
    }
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
