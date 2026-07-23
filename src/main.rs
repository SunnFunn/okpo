mod config;
mod discover;
mod schedule;
mod ssh;
mod transfer;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::config::Config;

#[derive(Debug, Parser)]
#[command(
    name = "okpo",
    about = "Ежедневная/ручная выгрузка реестров с UNC-шары на Ubuntu по SFTP (автопакет — 4 файла)"
)]
struct Cli {
    /// Один прогон с автопоиском пакета из 4 реестров (без ожидания расписания)
    #[arg(long, conflicts_with = "file")]
    once: bool,

    /// Ручная загрузка одного файла по имени (например: "Реестр 22.07..xls")
    #[arg(long, value_name = "NAME")]
    file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg = Config::load()?;

    if let Some(name) = cli.file.as_deref() {
        tracing::info!("ручная загрузка файла: {name}");
        schedule::run_job(&cfg, Some(name)).await?;
        return Ok(());
    }

    if cli.once {
        tracing::info!("разовый автопоиск пакета (4 файла) и загрузка");
        schedule::run_job(&cfg, None).await?;
        return Ok(());
    }

    schedule::run_daemon(cfg).await
}
