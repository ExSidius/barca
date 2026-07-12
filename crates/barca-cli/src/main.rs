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
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
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
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
    },
    /// Parse source files and emit the execution plan as JSON
    Plan {
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Environment name (accepted for symmetry; planning uses no state)
        #[arg(long)]
        env: Option<String>,
    },
    /// Show recent run history
    History {
        /// Number of recent runs to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
    },
    /// Show execution statistics for an asset
    Stats {
        /// Target asset function name
        target: String,
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
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
        /// Disable the cron scheduler (Schedule(...) assets will not auto-fire)
        #[arg(long)]
        no_schedule: bool,
        /// Timezone for cron evaluation: local (default), utc, or an IANA name
        #[arg(long, default_value = "local")]
        timezone: String,
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
    },
    /// List all discovered definitions (assets, tasks, sensors) with their deps
    ///
    /// Scheduled definitions also show their next fire time in local time.
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

    // The one runtime for the whole process — barca-core is async-native and
    // runs on whatever runtime the caller provides.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            eprintln!("failed to create runtime: {e}");
            std::process::exit(1);
        });
    let result = rt.block_on(run_cli(cli));

    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// A token that cancels on Ctrl-C, so an interrupted run terminates its
/// workers and is recorded as `cancelled` instead of lingering as `running`.
fn cancel_on_ctrl_c() -> barca_core::CancellationToken {
    let cancel = barca_core::CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            c.cancel();
        }
    });
    cancel
}

async fn run_cli(cli: Cli) -> Result<(), barca_core::BarcaError> {
    let python = barca_core::commands::find_python();

    match cli {
        Cli::Get {
            args,
            output,
            no_cache,
            agent,
            env,
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
            get_cmd(
                env.as_deref(),
                target,
                files,
                &python,
                output,
                no_cache,
                agent,
            )
            .await
        }
        Cli::Run {
            args,
            burst,
            output,
            agent,
            env,
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
            run_cmd(env.as_deref(), target, files, &python, burst, output, agent).await
        }
        Cli::Plan { files, env: _ } => plan_cmd(files, &python).await,
        Cli::History { limit, env } => history_cmd(env.as_deref(), limit).await,
        Cli::Stats { target, files, env } => {
            stats_cmd(env.as_deref(), target, files, &python).await
        }
        Cli::List { files } => list_cmd(files, &python).await,
        Cli::Serve {
            files,
            port,
            watch,
            no_schedule,
            timezone,
            env,
        } => {
            serve_cmd(
                env.as_deref(),
                files,
                port,
                watch,
                !no_schedule,
                timezone,
                &python,
            )
            .await
        }
        Cli::Version => {
            println!("barca {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn get_cmd(
    env: Option<&str>,
    target: Option<String>,
    files: Vec<PathBuf>,
    python: &PathBuf,
    mode: OutputMode,
    no_cache: bool,
    agent: bool,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::get(
        &cfg,
        target.as_deref(),
        &file_args,
        python,
        no_cache,
        agent,
        cancel_on_ctrl_c(),
    )
    .await?;
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

#[allow(clippy::too_many_arguments)]
async fn run_cmd(
    env: Option<&str>,
    target: String,
    files: Vec<PathBuf>,
    python: &PathBuf,
    burst: Option<Vec<String>>,
    mode: OutputMode,
    agent: bool,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::run(
        &cfg,
        &target,
        &file_args,
        python,
        burst,
        agent,
        cancel_on_ctrl_c(),
    )
    .await?;
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

async fn plan_cmd(files: Vec<PathBuf>, python: &PathBuf) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::plan(&file_args, python).await?;
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    Ok(())
}

async fn list_cmd(files: Vec<PathBuf>, python: &PathBuf) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let assets = barca_core::commands::list_assets(&file_args, python).await?;
    if assets.is_empty() {
        println!("No definitions found.");
        return Ok(());
    }

    // Next fire times for scheduled definitions (empty when nothing is scheduled,
    // so the NEXT FIRE column only appears when it carries information).
    let next_fires: std::collections::HashMap<String, String> =
        barca_server::describe_schedule(&file_args, python)
            .await
            .into_iter()
            .filter_map(|j| j.next_fire_local.map(|t| (j.id, t)))
            .collect();
    let has_schedule = !next_fires.is_empty();

    // Render each row's cells up front so column widths fit the actual content.
    let rows: Vec<(&str, String, String, &str, String)> = assets
        .iter()
        .map(|a| {
            let kind = serde_json::to_value(&a.kind)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| format!("{:?}", a.kind).to_lowercase());
            let freshness = serde_json::to_value(&a.freshness)
                .ok()
                .and_then(|v| {
                    let ty = v.get("type")?.as_str()?;
                    if ty == "Schedule" {
                        let cron = v.get("value").and_then(|c| c.as_str()).unwrap_or("?");
                        Some(format!("cron: {cron}"))
                    } else {
                        Some(ty.to_lowercase())
                    }
                })
                .unwrap_or_else(|| format!("{:?}", a.freshness).to_lowercase());
            let next = next_fires.get(&a.id).map(String::as_str).unwrap_or("-");
            let deps = if a.inputs.is_empty() {
                "-".to_string()
            } else {
                a.inputs.join(", ")
            };
            (a.id.as_str(), kind, freshness, next, deps)
        })
        .collect();

    let max_name = assets.iter().map(|a| a.id.len()).max().unwrap_or(4).max(4);
    let max_kind = rows.iter().map(|r| r.1.len()).max().unwrap_or(4).max(4);
    let max_fresh = rows.iter().map(|r| r.2.len()).max().unwrap_or(9).max(9); // "FRESHNESS"
    let max_next = rows.iter().map(|r| r.3.len()).max().unwrap_or(9).max(9); // "NEXT FIRE"

    if has_schedule {
        println!(
            "{:<wn$}  {:<wk$}  {:<ws$}  {:<wf$}  {}",
            "NAME",
            "KIND",
            "FRESHNESS",
            "NEXT FIRE",
            "DEPS",
            wn = max_name,
            wk = max_kind,
            ws = max_fresh,
            wf = max_next,
        );
        println!(
            "{}",
            "-".repeat(max_name + max_kind + max_fresh + max_next + 12)
        );
    } else {
        println!(
            "{:<wn$}  {:<wk$}  {:<ws$}  {}",
            "NAME",
            "KIND",
            "FRESHNESS",
            "DEPS",
            wn = max_name,
            wk = max_kind,
            ws = max_fresh,
        );
        println!("{}", "-".repeat(max_name + max_kind + max_fresh + 10));
    }

    for row in &rows {
        let (id, kind, freshness, next, deps) = row;
        if has_schedule {
            println!(
                "{:<wn$}  {:<wk$}  {:<ws$}  {:<wf$}  {}",
                id,
                kind,
                freshness,
                next,
                deps,
                wn = max_name,
                wk = max_kind,
                ws = max_fresh,
                wf = max_next,
            );
        } else {
            println!(
                "{:<wn$}  {:<wk$}  {:<ws$}  {}",
                id,
                kind,
                freshness,
                deps,
                wn = max_name,
                wk = max_kind,
                ws = max_fresh,
            );
        }
    }
    Ok(())
}

async fn history_cmd(env: Option<&str>, limit: usize) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let runs = barca_core::commands::history(&cfg, limit).await?;
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

async fn stats_cmd(
    env: Option<&str>,
    target: String,
    files: Vec<PathBuf>,
    python: &PathBuf,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let stats = barca_core::commands::stats(&cfg, &target, &file_args, python).await?;
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

#[allow(clippy::too_many_arguments)]
async fn serve_cmd(
    env: Option<&str>,
    files: Vec<PathBuf>,
    port: u16,
    watch: bool,
    schedule: bool,
    timezone: String,
    python: &std::path::Path,
) -> Result<(), barca_core::BarcaError> {
    let resolved = barca_core::config::resolve(env)?;
    if resolved.state == barca_core::config::StateMode::Optimistic && resolved.state_uri.is_some() {
        return Err(barca_core::BarcaError::Other(
            "barca serve does not support shared remote state yet — set state = \"off\" \
             in barca.toml (or BARCA_STATE=off) to serve with a local metadata DB"
                .to_string(),
        ));
    }
    let config = barca_server::ServeConfig {
        files: files.iter().map(|p| p.display().to_string()).collect(),
        host: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        port,
        watch,
        schedule,
        timezone,
        python: python.to_path_buf(),
        resolved,
    };
    barca_server::serve(config)
        .await
        .map_err(|e| barca_core::BarcaError::Other(e.to_string()))
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
