use code::cli::cli_main;
use tokio::main;

#[main]
async fn main() -> anyhow::Result<()> {
    cli_main().await
}
