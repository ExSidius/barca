//! Barca CLI — invisible asset orchestrator.

mod docs;

use clap::{Parser, ValueEnum};
use std::io::Write;
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

// ─── Help text ────────────────────────────────────────────────────────────────
//
// Every command carries runnable examples. The tests at the bottom of this file parse each
// `barca ...` example line against the real CLI, so these cannot drift from the flags.
// When you add or change a flag, update the examples here and the matching `barca docs`
// topic in crates/barca-cli/docs/.

const TOP_HELP: &str = "\
Quick start:
  barca list pipeline.py            # discover assets, tasks and their dependencies
  barca get total pipeline.py       # run only what `total` needs (cached on re-run)
  barca run deploy pipeline.py      # run a task and its dependency cone
  barca docs                        # built-in manual: concepts, formats, examples

Output: results are JSON on stdout; progress and errors go to stderr. Exit codes: 0 ok,
1 runtime failure, 2 usage error. Scripts and AI agents: barca docs agents";

const GET_HELP: &str = "\
Examples:
  barca get pipeline.py                    # every asset in the file; prints the last one's value
  barca get total pipeline.py              # one target and only its upstream cone
  barca get total pipeline.py other.py     # target defined across several files
  barca get total pipeline.py --no-cache   # recompute everything in that cone
  barca get total pipeline.py --dry-run    # what would run vs come from cache; changes nothing
  barca get total pipeline.py -o value     # just the value, pretty-printed
  barca get total pipeline.py --agent      # plain progress lines on stderr
  barca get total pipeline.py --env dev    # separate cache and state per environment

Output: one JSON line on stdout with run_id, steps_executed (0 = all cached), phases and
final_output, and `steps`: what happened to each step (ran or cached, and why). For parquet/pickle assets final_output is a pointer,
{\"_barca_artifact\": {\"path\", \"format\", \"size_bytes\"}}; the Python API (barca.get)
loads the value for you.
Targets must be assets; use `barca run` for tasks.
More: barca docs cache, barca docs types, barca docs agents";

const RUN_HELP: &str = "\
Examples:
  barca run deploy pipeline.py                         # task runs; upstream assets come from cache
  barca run deploy pipeline.py --refresh fetch,clean   # also re-materialize these upstream assets
  barca run deploy pipeline.py --refresh-all           # re-materialize every upstream asset
  barca run deploy pipeline.py --no-cache              # same as --refresh-all
  barca run deploy pipeline.py --dry-run --refresh fetch   # preview: which steps run, which are cached

--refresh takes ONE comma-separated list (`--refresh a,b`), never `--refresh a b`. It re-runs only
the assets you name: assets downstream of them stay cached unless you list them too (barca prints
a warning when that happens). A name that is not an upstream asset is an error.
The target must be a task; use `barca get` for assets.
More: barca docs tasks, barca docs cache";

const PLAN_HELP: &str = "\
Examples:
  barca plan pipeline.py              # phases and steps that would run; nothing executes
  barca plan pipeline.py other.py     # several files form one DAG

Output: pretty-printed JSON {total_steps, phases: [{reason, streams: [{stream_id, steps}]}]}.
Planning is static analysis: it never imports your code.
More: barca docs agents";

const HISTORY_HELP: &str = "\
Examples:
  barca history                # last 10 runs as a table
  barca history -l 25          # last 25
  barca history --json         # machine-readable array of runs
  barca history --env dev      # runs recorded in another environment

More: barca docs cache";

const STATS_HELP: &str = "\
Examples:
  barca stats total pipeline.py             # timing percentiles and cache hit rate
  barca stats total pipeline.py --json      # the same as one JSON object

More: barca docs cache";

const SERVE_HELP: &str = "\
Examples:
  barca serve pipeline.py                    # HTTP API on 127.0.0.1:8274 plus the scheduler
  barca serve pipeline.py --port 8400        # custom port
  barca serve pipeline.py --watch            # dev: re-parse the DAG when files change
  barca serve pipeline.py --no-schedule      # API only; Schedule(...) nodes do not fire
  barca serve pipeline.py --timezone utc     # evaluate cron in UTC (default: local)

Binds to localhost with no authentication.
More: barca docs scheduling";

const LIST_HELP: &str = "\
Examples:
  barca list pipeline.py             # table of nodes: kind, freshness, dependencies
  barca list pipeline.py --json      # array of {id, kind, freshness, inputs, next_fire?}
  barca list a.py b.py               # several files form one DAG

Run this first to confirm barca discovered your nodes.
More: barca docs assets, barca docs agents";

const DOCS_HELP: &str = "\
Examples:
  barca docs                    # topic index with one-line summaries
  barca docs types              # one topic as markdown
  barca docs examples/duckdb    # a runnable example pipeline
  barca docs --all              # the whole manual in one stream (paste into context)
  barca docs --json             # topic index as JSON
  barca docs cache --json       # one topic as JSON {name, summary, content}

Topics are compiled into the binary: offline, and always matching this version.";

#[derive(Parser)]
#[command(
    name = "barca",
    about = "Invisible asset orchestrator",
    long_about = "Barca runs Python asset graphs with content-addressed caching.\n\
                  Every asset output is fully materialized to an artifact file at step \
                  boundaries (json, pickle, or parquet) — that persistence is the cache \
                  checkpoint. pandas/polars DataFrames, pyarrow Tables and duckdb relations \
                  are written as parquet; parameter type annotations choose how downstream \
                  steps read it back (pandas by default, or polars, pyarrow, duckdb) but do \
                  not skip materialization. Run `barca docs` for the manual.",
    after_help = TOP_HELP,
    version
)]
enum Cli {
    /// Get asset value(s) — cache-aware, runs only the needed subgraph
    ///
    /// If the first positional arg ends in .py, all args are treated as files
    /// (no target — gets all assets). Otherwise, the first arg is the target
    /// asset name and the rest are files.
    ///
    /// Each completed step writes a fully materialized artifact (never a lazy in-memory
    /// handle). If one computation should produce several cacheable outputs, define
    /// multiple assets or split the work inside a single step before returning.
    #[command(after_help = GET_HELP)]
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
        /// Show what this command would do (each step cached or will-run, and why) without
        /// running or writing anything
        #[arg(long)]
        dry_run: bool,
        /// Agent-friendly output: plain structured progress lines instead of visual progress bar
        #[arg(long)]
        agent: bool,
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
    },
    /// Run a task and its dependency cone — the task always re-runs
    ///
    /// The task always re-runs. Upstream assets are served from cache when fresh
    /// (same as `barca get`). Use `--refresh` to force re-materialize specific
    /// upstream assets, or `--refresh-all` / `--no-cache` to refresh the entire
    /// upstream cone.
    #[command(after_help = RUN_HELP)]
    Run {
        /// TARGET file.py [file.py ...] — target task is required
        #[arg(required = true)]
        args: Vec<String>,
        /// Upstream assets to force re-materialize, as ONE comma-separated list
        /// (`--refresh a,b`, not `--refresh a b`). Assets downstream of them stay cached
        /// unless also listed; barca warns when that happens
        #[arg(long, value_delimiter = ',', conflicts_with = "refresh_all")]
        refresh: Option<Vec<String>>,
        /// Force re-materialize ALL upstream assets in the task's cone
        #[arg(long, alias = "no-cache")]
        refresh_all: bool,
        /// Show what this command would do (each step cached or will-run, and why) without
        /// running or writing anything
        #[arg(long)]
        dry_run: bool,
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
    #[command(after_help = PLAN_HELP)]
    Plan {
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Environment name (accepted for symmetry; planning uses no state)
        #[arg(long)]
        env: Option<String>,
    },
    /// Show recent run history
    #[command(after_help = HISTORY_HELP)]
    History {
        /// Number of recent runs to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Emit JSON (an array of runs) instead of a table
        #[arg(long)]
        json: bool,
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
    },
    /// Show execution statistics for an asset
    #[command(after_help = STATS_HELP)]
    Stats {
        /// Target asset function name
        target: String,
        /// Python source files containing @asset definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
        /// Environment name (separates cache/state per environment)
        #[arg(long)]
        env: Option<String>,
    },
    /// Run a long-running HTTP server exposing the orchestrator as a JSON API
    ///
    /// Binds to 127.0.0.1 (local only, no auth). POST /run and /get trigger
    /// async runs; poll GET /status/<run_id> for results.
    #[command(after_help = SERVE_HELP)]
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
    #[command(after_help = LIST_HELP)]
    List {
        /// Python source files containing definitions
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Emit JSON (an array of nodes) instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Show the built-in manual: concepts, output formats, examples, agent conventions
    ///
    /// Topics are compiled into the binary, so this works offline and always matches the
    /// installed version. With no topic it prints an index.
    #[command(after_help = DOCS_HELP)]
    Docs {
        /// Topic to show (omit for the index), e.g. types, cache, examples/duckdb
        topic: Option<String>,
        /// Print every topic in one stream
        #[arg(long, conflicts_with = "topic")]
        all: bool,
        /// Emit JSON instead of markdown
        #[arg(long)]
        json: bool,
    },
    /// Print version information
    Version,
}

/// Reject file arguments that are not `.py` files, with a hint for the most common mistake:
/// passing several assets to `--refresh` separated by spaces instead of commas.
fn check_py_files(files: &[PathBuf], refresh: Option<&[String]>) {
    let Some(bad) = files.iter().find(|f| !f.to_string_lossy().ends_with(".py")) else {
        return;
    };
    let bad = bad.to_string_lossy();
    eprintln!("error: '{bad}' is not a .py file.");
    if let Some(names) = refresh {
        let mut all: Vec<String> = names.to_vec();
        all.push(bad.to_string());
        eprintln!(
            "\nIf you meant to refresh several assets, join them with commas: --refresh {}",
            all.join(",")
        );
    }
    std::process::exit(1);
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

    // Version needs no runtime — answer before paying for thread spawns.
    if let Cli::Version = cli {
        println!("barca {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // The manual is compiled in: no runtime, no Python, no project files needed.
    if let Cli::Docs { topic, all, json } = &cli {
        match docs::run(topic.as_deref(), *all, *json) {
            // Ignore write errors (e.g. a closed pipe from `barca docs --all | head`).
            Ok(out) => {
                let _ = std::io::stdout().lock().write_all(out.as_bytes());
            }
            Err(msg) => {
                eprintln!("{msg}");
                std::process::exit(1);
            }
        }
        return;
    }

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
            dry_run,
            agent,
            env,
        } => {
            let (target, files) = split_target_files(args);
            check_py_files(&files, None);
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
                dry_run,
                agent,
            )
            .await
        }
        Cli::Run {
            args,
            refresh,
            refresh_all,
            dry_run,
            output,
            agent,
            env,
        } => {
            let (target, files) = split_target_files(args);
            check_py_files(&files, refresh.as_deref());
            let Some(target) = target else {
                eprintln!(
                    "error: a target task is required\n\nUsage: barca run <TARGET> <FILES>... [--refresh a,b | --refresh-all]"
                );
                std::process::exit(1);
            };
            if files.is_empty() {
                eprintln!(
                    "error: no .py files provided\n\nUsage: barca run <TARGET> <FILES>... [--refresh a,b | --refresh-all]"
                );
                std::process::exit(1);
            }
            let policy = match (refresh_all, refresh) {
                (true, _) => barca_core::commands::CachePolicy::RefreshAll,
                (false, Some(names)) => barca_core::commands::CachePolicy::RefreshSelective(names),
                (false, None) => barca_core::commands::CachePolicy::CacheAware,
            };
            run_cmd(
                env.as_deref(),
                target,
                files,
                &python,
                policy,
                dry_run,
                output,
                agent,
            )
            .await
        }
        Cli::Plan { files, env: _ } => plan_cmd(files, &python).await,
        Cli::History { limit, json, env } => history_cmd(env.as_deref(), limit, json).await,
        Cli::Stats {
            target,
            files,
            json,
            env,
        } => stats_cmd(env.as_deref(), target, files, json, &python).await,
        Cli::List { files, json } => list_cmd(files, json, &python).await,
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
        // Answered in main() before the runtime is built — never reaches here.
        Cli::Version => unreachable!("version is handled before runtime construction"),
        Cli::Docs { .. } => unreachable!("docs is handled before runtime construction"),
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
    dry_run: bool,
    agent: bool,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    if dry_run {
        let policy = barca_core::commands::CachePolicy::CacheAware;
        return explain_cmd(
            &cfg, target, &file_args, python, policy, no_cache, "get", mode,
        )
        .await;
    }
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
                    "steps": &result.steps,
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
    policy: barca_core::commands::CachePolicy,
    dry_run: bool,
    mode: OutputMode,
    agent: bool,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    if dry_run {
        return explain_cmd(
            &cfg,
            Some(target),
            &file_args,
            python,
            policy,
            false,
            "run",
            mode,
        )
        .await;
    }
    let result = barca_core::commands::run(
        &cfg,
        &target,
        &file_args,
        python,
        policy,
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
                    "steps": &result.steps,
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

#[allow(clippy::too_many_arguments)]
async fn explain_cmd(
    cfg: &barca_core::config::ResolvedConfig,
    target: Option<String>,
    file_args: &[String],
    python: &PathBuf,
    policy: barca_core::commands::CachePolicy,
    no_cache: bool,
    label: &str,
    mode: OutputMode,
) -> Result<(), barca_core::BarcaError> {
    let result = barca_core::commands::explain(
        cfg,
        target.as_deref(),
        file_args,
        python,
        policy,
        no_cache,
        label,
    )
    .await?;
    match mode {
        OutputMode::Json => println!("{}", serde_json::to_string(&result).unwrap()),
        OutputMode::Value => println!("{}", serde_json::to_string_pretty(&result.steps).unwrap()),
        OutputMode::Pretty => {
            println!(
                "Dry run: barca {label}{} (nothing executed, nothing written)\n",
                result
                    .target
                    .as_deref()
                    .map(|t| format!(" {t}"))
                    .unwrap_or_default()
            );
            print_step_table(&result.steps, true);
            println!(
                "\n{} will run, {} cached, {} unknown",
                result.summary.will_run, result.summary.cached, result.summary.unknown
            );
        }
    }
    Ok(())
}

/// STATUS / WHY / STEP table for dry runs and (in `-o pretty`) real runs.
fn print_step_table(steps: &[barca_core::commands::StepReport], dry: bool) {
    let rows: Vec<(String, String, &str)> = steps
        .iter()
        .map(|s| {
            let verdict = s.action.as_deref().or(s.status.as_deref()).unwrap_or("?");
            let label = match (dry, verdict) {
                (true, "run") => "will run",
                (_, v) => v,
            };
            let why = match (&s.partitions, &s.detail) {
                (Some(p), _) if verdict == "partial" => format!(
                    "{} of {} keys cached; will run: {}",
                    p.cached,
                    p.total,
                    p.will_run_keys.join(", ")
                ),
                (Some(p), Some(d)) => format!("{} keys; {d}", p.total),
                (None, Some(d)) => d.clone(),
                (Some(p), None) => format!("{} keys cached", p.total),
                (None, None) => "-".to_string(),
            };
            (label.to_string(), why, s.id.as_str())
        })
        .collect();
    let w_status = rows.iter().map(|r| r.0.len()).max().unwrap_or(6).max(6);
    let w_why = rows
        .iter()
        .map(|r| r.1.len())
        .max()
        .unwrap_or(3)
        .clamp(3, 70);
    println!("{:<w_status$}  {:<w_why$}  STEP", "STATUS", "WHY");
    for (label, why, id) in &rows {
        println!("{label:<w_status$}  {why:<w_why$}  {id}");
    }
    for s in steps {
        if let Some(w) = &s.warning {
            println!("\n  ! {w}");
        }
    }
}

async fn plan_cmd(files: Vec<PathBuf>, python: &PathBuf) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let result = barca_core::commands::plan(&file_args, python).await?;
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    Ok(())
}

async fn list_cmd(
    files: Vec<PathBuf>,
    json: bool,
    python: &PathBuf,
) -> Result<(), barca_core::BarcaError> {
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let assets = barca_core::commands::list_assets(&file_args, python).await?;
    if json {
        // Machine-readable: every node, plus `next_fire` (local time) for scheduled ones.
        let next_fires: std::collections::HashMap<String, String> =
            barca_server::describe_schedule(&file_args, python)
                .await
                .into_iter()
                .filter_map(|j| j.next_fire_local.map(|t| (j.id, t)))
                .collect();
        let nodes: Vec<serde_json::Value> = assets
            .iter()
            .map(|a| {
                let mut v = serde_json::to_value(a).unwrap_or(serde_json::Value::Null);
                if let (Some(obj), Some(t)) = (v.as_object_mut(), next_fires.get(&a.id)) {
                    obj.insert("next_fire".into(), serde_json::Value::String(t.clone()));
                }
                v
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&nodes).unwrap());
        return Ok(());
    }
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

async fn history_cmd(
    env: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let runs = barca_core::commands::history(&cfg, limit).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&runs).unwrap());
        return Ok(());
    }
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
    json: bool,
    python: &PathBuf,
) -> Result<(), barca_core::BarcaError> {
    let cfg = barca_core::config::resolve(env)?;
    let file_args: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let stats = barca_core::commands::stats(&cfg, &target, &file_args, python).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&stats).unwrap());
        return Ok(());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Subcommands that must carry runnable examples in their `--help`.
    const DOCUMENTED: &[&str] = &[
        "get", "run", "plan", "history", "stats", "serve", "list", "docs",
    ];

    fn after_help(cmd: &clap::Command) -> String {
        cmd.get_after_help()
            .or_else(|| cmd.get_after_long_help())
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// `barca ...` command lines found in `text`: indented example lines in help text, or
    /// lines inside ```bash fences in a docs topic. Trailing `# comments` are stripped.
    fn command_lines(text: &str, only_in_bash_fences: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut in_bash = false;
        for line in text.lines() {
            let t = line.trim();
            if only_in_bash_fences {
                if t.starts_with("```") {
                    in_bash = t == "```bash";
                    continue;
                }
                if !in_bash {
                    continue;
                }
            }
            if t.starts_with("barca ") {
                let cmd = t.split(" #").next().unwrap_or(t).trim();
                out.push(cmd.to_string());
            }
        }
        out
    }

    fn assert_parses(cmd_line: &str, ctx: &str) {
        let argv = cmd_line.split_whitespace();
        match Cli::try_parse_from(argv) {
            Ok(_) => {}
            // `--help` / `--version` "fail" with a display error; that is a valid command.
            Err(e)
                if matches!(
                    e.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) => {}
            Err(e) => panic!("{ctx}: `{cmd_line}` does not parse:\n{e}"),
        }
    }

    #[test]
    fn every_documented_subcommand_has_examples_and_a_docs_pointer() {
        let root = Cli::command();
        for name in DOCUMENTED {
            let sub = root
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("missing subcommand {name}"));
            let help = after_help(sub);
            assert!(
                help.contains("Examples:"),
                "`barca {name} --help` needs an `Examples:` section (after_help)"
            );
            assert!(
                !command_lines(&help, false).is_empty(),
                "`barca {name} --help` examples need at least one `barca ...` line"
            );
        }
    }

    #[test]
    fn top_level_help_points_at_the_manual() {
        let help = after_help(&Cli::command());
        assert!(
            help.contains("barca docs"),
            "top-level --help must mention `barca docs`"
        );
        assert!(
            help.contains("barca docs agents"),
            "top-level --help must point agents at `barca docs agents`"
        );
    }

    #[test]
    fn every_help_example_parses_against_the_real_cli() {
        let root = Cli::command();
        for name in DOCUMENTED {
            let help = after_help(root.find_subcommand(name).unwrap());
            for line in command_lines(&help, false) {
                assert_parses(&line, &format!("`barca {name} --help` example"));
            }
        }
        for line in command_lines(&after_help(&root), false) {
            assert_parses(&line, "top-level --help example");
        }
    }

    #[test]
    fn every_docs_topic_command_parses_against_the_real_cli() {
        for t in docs::TOPICS {
            for line in command_lines(t.body, true) {
                assert_parses(&line, &format!("docs topic '{}'", t.name));
            }
        }
    }

    #[test]
    fn docs_pointers_in_help_text_resolve_to_topics() {
        let root = Cli::command();
        let mut texts = vec![after_help(&root)];
        for name in DOCUMENTED {
            texts.push(after_help(root.find_subcommand(name).unwrap()));
        }
        for text in texts {
            for topic in docs::referenced_topics(&text) {
                assert!(
                    docs::find(&topic).is_some(),
                    "--help mentions unknown `barca docs {topic}`"
                );
            }
        }
    }

    #[test]
    fn every_flag_and_argument_has_help_text() {
        fn check(cmd: &clap::Command, path: &str) {
            for arg in cmd.get_arguments() {
                if arg.is_hide_set() || matches!(arg.get_id().as_str(), "help" | "version") {
                    continue;
                }
                assert!(
                    arg.get_help().is_some() || arg.get_long_help().is_some(),
                    "`{path}` argument '{}' has no help text — add a doc comment",
                    arg.get_id()
                );
            }
            for sub in cmd.get_subcommands() {
                check(sub, &format!("{path} {}", sub.get_name()));
            }
        }
        check(&Cli::command(), "barca");
    }

    #[test]
    fn every_subcommand_has_a_one_line_description() {
        for sub in Cli::command().get_subcommands() {
            assert!(
                sub.get_about().is_some(),
                "`barca {}` has no description",
                sub.get_name()
            );
        }
    }

    #[test]
    fn json_flags_exist_on_inspection_commands() {
        let root = Cli::command();
        for name in ["list", "history", "stats", "docs"] {
            let sub = root.find_subcommand(name).unwrap();
            assert!(
                sub.get_arguments().any(|a| a.get_id() == "json"),
                "`barca {name}` needs a --json flag for machine-readable output"
            );
        }
    }
}
