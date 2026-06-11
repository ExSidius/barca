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
    /// Get asset value(s) — cache-aware, runs only the needed subgraph
    ///
    /// If the first positional arg ends in .py, all args are treated as files
    /// (no target — gets all assets). Otherwise, the first arg is the target
    /// asset name and the rest are files.
    Get {
        /// [TARGET] file.py [file.py ...] — target is optional
        #[arg(required = true)]
        args: Vec<String>,
        /// Output format
        #[arg(short, long, default_value = "json")]
        output: OutputMode,
        /// Skip cache — execute everything fresh
        #[arg(long)]
        no_cache: bool,
        /// Agent-friendly output: plain structured progress lines instead of visual progress bar
        #[arg(long)]
        agent: bool,
    },
    /// Run a task (and its cone) — always re-runs, bursting upstream asset caches
    ///
    /// Like `get`, but for task-style workflows: tasks always execute, and by
    /// default every upstream asset is force-rerun. Use `--burst` to re-run only
    /// selected assets while the rest stay cached.
    Run {
        /// TARGET file.py [file.py ...] — target task is required
        #[arg(required = true)]
        args: Vec<String>,
        /// Comma-separated asset names to force-rerun. Omit to burst ALL upstream assets.
        #[arg(long, value_delimiter = ',')]
        burst: Option<Vec<String>>,
        /// Output format
        #[arg(short, long, default_value = "json")]
        output: OutputMode,
        /// Agent-friendly output: plain structured progress lines instead of visual progress bar
        #[arg(long)]
        agent: bool,
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
    /// Run a long-running HTTP server exposing the orchestrator as a JSON API
    ///
    /// Binds to 127.0.0.1 (local only, no auth). POST /run and /get trigger
    /// async runs; poll GET /status/<run_id> for results.
    Serve {
        /// Python source files defining the DAG to serve
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Port to bind on
        #[arg(short, long, default_value = "8274")]
        port: u16,
        /// Dev mode: re-parse the DAG when source files change
        #[arg(long)]
        watch: bool,
    },
    /// List all discovered definitions (assets, tasks, sensors) with their deps
    List {
        /// Python source files containing definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Print version information
    Version,
}

/// Split the raw positional args into (optional target, files).
/// If the first arg ends in `.py`, all args are files (no target).
/// Otherwise, the first arg is the target and the rest are files.
fn split_target_files(args: Vec<String>) -> (Option<String>, Vec<PathBuf>) {
    if args.is_empty() {
        return (None, Vec::new());
    }
    if args[0].ends_with(".py") {
        // All args are files.
        let files = args.into_iter().map(PathBuf::from).collect();
        (None, files)
    } else {
        // First arg is the target, rest are files.
        let target = args[0].clone();
        let files = args[1..].iter().map(PathBuf::from).collect();
        (Some(target), files)
    }
}

fn main() {
    // Support `barca file.py [--flags]` as shorthand for `barca get file.py [--flags]`.
    let cli = Cli::try_parse().unwrap_or_else(|_| {
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 && !args[1].starts_with('-') && args[1].ends_with(".py") {
            // Insert "get" after the program name so clap handles all flags.
            let mut rewritten = vec![args[0].clone(), "get".to_string()];
            rewritten.extend_from_slice(&args[1..]);
            Cli::parse_from(rewritten)
        } else {
            Cli::parse() // re-parse to show proper clap error
        }
    });
    let python = barca_core::commands::find_python();

    let result = match cli {
        Cli::Get {
            args,
            output,
            no_cache,
            agent,
        } => {
            let (target, files) = split_target_files(args);
            if files.is_empty() && target.is_none() {
                eprintln!("error: no files provided\n\nUsage: barca get [TARGET] <FILES>...");
                std::process::exit(1);
            }
            if files.is_empty() && target.is_some() {
                eprintln!("error: no .py files provided\n\nUsage: barca get [TARGET] <FILES>...");
                std::process::exit(1);
            }
            get_cmd(target, files, &python, output, no_cache, agent)
        }
        Cli::Run {
            args,
            burst,
            output,
            agent,
        } => {
            let (target, files) = split_target_files(args);
            let Some(target) = target else {
                eprintln!(
                    "error: a target task is required\n\nUsage: barca run <TARGET> <FILES>... [--burst a,b]"
                );
                std::process::exit(1);
            };
            if files.is_empty() {
                eprintln!(
                    "error: no .py files provided\n\nUsage: barca run <TARGET> <FILES>... [--burst a,b]"
                );
                std::process::exit(1);
            }
            run_cmd(target, files, &python, burst, output, agent)
        }
        Cli::Plan { files } => plan_cmd(files, &python),
        Cli::History { limit } => history_cmd(limit),
        Cli::Stats { target, files } => stats_cmd(target, files, &python),
        Cli::List { files } => list_cmd(files, &python),
        Cli::Serve { files, port, watch } => serve_cmd(files, port, watch, &python),
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

#[allow(clippy::too_many_arguments)]
fn get_cmd(
    target: Option<String>,
    files: Vec<PathBuf>,
    python: &PathBuf,
    mode: OutputMode,
    no_cache: bool,
    agent: bool,
) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::get(target.as_deref(), &file_args, python, no_cache, agent)?;
    let final_output = result.final_output.as_ref().map(read_final_output);

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
            let label = target
                .as_ref()
                .map(|t| format!("got '{t}'"))
                .unwrap_or_else(|| "all assets".to_string());
            println!(
                "Run {} | {} in {:.3}s ({} step{}, {} phase{})",
                result.run_id,
                label,
                result.elapsed_seconds,
                result.steps_executed,
                if result.steps_executed == 1 { "" } else { "s" },
                result.phases,
                if result.phases == 1 { "" } else { "s" }
            );
            if let Some(ref val) = final_output {
                println!("\nValue:\n{}", serde_json::to_string_pretty(val).unwrap());
            }
        }
    }
    Ok(())
}

fn run_cmd(
    target: String,
    files: Vec<PathBuf>,
    python: &PathBuf,
    burst: Option<Vec<String>>,
    mode: OutputMode,
    agent: bool,
) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::run(&target, &file_args, python, burst, agent)?;
    let final_output = result.final_output.as_ref().map(read_final_output);

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
                "Run {} | ran '{}' in {:.3}s ({} step{}, {} phase{})",
                result.run_id,
                target,
                result.elapsed_seconds,
                result.steps_executed,
                if result.steps_executed == 1 { "" } else { "s" },
                result.phases,
                if result.phases == 1 { "" } else { "s" }
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

fn list_cmd(files: Vec<PathBuf>, python: &PathBuf) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let assets = barca_core::commands::list_assets(&file_args, python)?;
    if assets.is_empty() {
        println!("No definitions found.");
        return Ok(());
    }
    let max_name = assets.iter().map(|a| a.id.len()).max().unwrap_or(4).max(4);
    let max_kind = 6; // "sensor" is the longest
    println!(
        "{:<wn$}  {:<wk$}  {:<10}  {}",
        "NAME",
        "KIND",
        "FRESHNESS",
        "DEPS",
        wn = max_name,
        wk = max_kind,
    );
    println!("{}", "-".repeat(max_name + max_kind + 20));
    for a in &assets {
        let kind = serde_json::to_value(&a.kind)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", a.kind).to_lowercase());
        let freshness = serde_json::to_value(&a.freshness)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str().map(String::from)))
            .unwrap_or_else(|| format!("{:?}", a.freshness));
        let deps = if a.inputs.is_empty() {
            "-".to_string()
        } else {
            a.inputs.join(", ")
        };
        println!(
            "{:<wn$}  {:<wk$}  {:<10}  {}",
            a.id,
            kind,
            freshness,
            deps,
            wn = max_name,
            wk = max_kind,
        );
    }
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
    let fmt = |v: Option<f64>| v.map(|e| format!("{:.3}s", e)).unwrap_or("-".to_string());
    println!("Asset: {}", stats.node_id);
    println!("Total materializations: {}", stats.total_runs);
    println!(
        "Timing:  avg {}  median {}  p95 {}  max {}",
        fmt(stats.avg_elapsed_seconds),
        fmt(stats.median_elapsed_seconds),
        fmt(stats.p95_elapsed_seconds),
        fmt(stats.max_elapsed_seconds),
    );
    println!("Cache hit rate: {:.1}%", stats.cache_hit_rate * 100.0);
    if !stats.recent_runs.is_empty() {
        println!("\nRecent runs:");
        println!(
            "  {:<10} {:<9} {:<8} {:<20}",
            "ELAPSED", "STATUS", "ATTEMPTS", "CREATED"
        );
        for entry in &stats.recent_runs {
            let elapsed_str = entry
                .elapsed_seconds
                .map(|e| format!("{:.3}s", e))
                .unwrap_or_else(|| "-".to_string());
            println!(
                "  {:<10} {:<9} {:<8} {:<20}",
                elapsed_str, entry.status, entry.attempts, entry.created_at,
            );
            if entry.status == "failed"
                && let Some(msg) = &entry.error_message
                && !msg.is_empty()
            {
                println!("      └─ {msg}");
            }
        }
    }
    Ok(())
}

fn serve_cmd(
    files: Vec<PathBuf>,
    port: u16,
    watch: bool,
    python: &std::path::Path,
) -> Result<(), barca_core::BarcaError> {
    let config = barca_server::ServeConfig {
        files: files.iter().map(|p| p.display().to_string()).collect(),
        host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
        watch,
        python: python.to_path_buf(),
    };
    barca_server::serve(config).map_err(|e| barca_core::BarcaError::Other(e.to_string()))
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
