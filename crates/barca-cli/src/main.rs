#[tokio::main]
async fn main() -> anyhow::Result<()> {
    barca_cli::run().await
}
