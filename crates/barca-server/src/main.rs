use anyhow::Context;
use clap::{Parser, Subcommand};

use barca_server::{config::load_config, server, AppState};

#[derive(Parser)]
#[command(name = "barca", about = "Minimal asset orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the barca server (default)
    Serve,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Serve) => serve().await,
        Some(Command::Reset { db, artifacts, tmp }) => {
            let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
            let output = barca_server::reset(&repo_root, db, artifacts, tmp)?;
            print!("{output}");
            Ok(())
        }
    }
}

async fn serve() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let config = load_config(&repo_root.join("barca.toml"))?;
    let store =
        barca_server::store::MetadataStore::open(&repo_root.join(".barca").join("metadata.db"))
            .await?;
    let python = barca_server::python_bridge::PythonBridge::new(repo_root.clone());

    let state = AppState::new(repo_root, config, store, python);

    barca_server::reindex(&state).await?;
    {
        let store = state.store.lock().await;
        store.requeue_running_materializations().await?;
    }
    tracing::info!("refresh queue recovery complete");
    tokio::spawn(barca_server::run_refresh_queue_worker(state.clone()));

    let app = server::router().with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!("barca listening on http://127.0.0.1:3000");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}
