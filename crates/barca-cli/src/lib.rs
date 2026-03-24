pub mod commands;
pub mod display;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "barca",
    about = "Minimal asset orchestrator",
    after_help = "\
Examples:
  barca                        Start the server (default)
  barca serve                  Start the server
  barca reindex                Re-inspect Python modules
  barca assets list            List all indexed assets
  barca assets show <id>       Show asset details
  barca assets refresh <id>    Trigger materialization
  barca jobs list              List recent jobs
  barca jobs show <id>         Show job details
  barca reset --db             Remove metadata database"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Maximum number of concurrent worker subprocesses (default: number of CPU cores)
    #[arg(long, short = 'j', global = true)]
    concurrency: Option<usize>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the barca server
    Serve,
    /// Re-inspect Python modules and update the asset index
    Reindex,
    /// Remove generated files and caches
    Reset {
        /// Only remove the metadata database (.barca/)
        #[arg(long)]
        db: bool,
        /// Only remove materialized artifacts (.barcafiles/)
        #[arg(long)]
        artifacts: bool,
        /// Only remove temporary staging files (tmp/)
        #[arg(long)]
        tmp: bool,
    },
    /// Manage assets (list, show, refresh)
    #[command(subcommand)]
    Assets(AssetsCmd),
    /// Manage jobs (list, show)
    #[command(subcommand)]
    Jobs(JobsCmd),
}

#[derive(Subcommand)]
enum AssetsCmd {
    /// List all indexed assets
    List,
    /// Show asset details
    Show {
        /// Asset ID
        id: i64,
    },
    /// Trigger materialization and wait for completion
    Refresh {
        /// Asset ID
        id: i64,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    /// List recent materialization jobs
    List,
    /// Show job details
    Show {
        /// Job ID
        id: i64,
    },
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Reset is the only command that doesn't need tracing or reindex
    if matches!(cli.command, Some(Commands::Reset { .. })) {
        if let Some(Commands::Reset { db, artifacts, tmp }) = cli.command {
            return commands::reset_cmd(db, artifacts, tmp);
        }
    }

    tracing_subscriber::fmt().with_env_filter("info").init();

    let concurrency = cli.concurrency;

    match cli.command {
        None | Some(Commands::Serve) => commands::serve(concurrency).await,
        Some(Commands::Reindex) => commands::reindex_cmd().await,
        Some(Commands::Assets(sub)) => match sub {
            AssetsCmd::List => commands::assets_list().await,
            AssetsCmd::Show { id } => commands::assets_show(id).await,
            AssetsCmd::Refresh { id } => commands::assets_refresh(id, concurrency).await,
        },
        Some(Commands::Jobs(sub)) => match sub {
            JobsCmd::List => commands::jobs_list().await,
            JobsCmd::Show { id } => commands::jobs_show(id).await,
        },
        Some(Commands::Reset { .. }) => unreachable!(),
    }
}
