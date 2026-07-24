mod agent;
mod config;
mod discover;
mod logging;
mod schedule;
mod ssh;
mod transfer;

use anyhow::Result;
use clap::Parser;

use crate::config::Config;
use crate::logging::LogFile;

#[derive(Debug, Parser)]
#[command(
    name = "okpo",
    about = "Ежедневная/ручная выгрузка реестров с UNC-шары на Ubuntu по SFTP + запуск okpo-agent (SSH -R SOCKS)"
)]
struct Cli {
    /// Один прогон с автопоиском пакета из 4 реестров (без ожидания расписания)
    #[arg(long, conflicts_with = "file")]
    once: bool,

    /// Ручная загрузка одного файла по имени (например: "Реестр 22.07..xls")
    #[arg(long, value_name = "NAME")]
    file: Option<String>,

    /// Только SFTP, без `ssh -R` и без запуска okpo-agent на Ubuntu
    #[arg(long)]
    skip_agent: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = logging::init()?;
    let cli = Cli::parse();

    if cli.file.is_some() || cli.once {
        begin_run(&log_file)?;
        let cfg = Config::load()?;

        if let Some(name) = cli.file.as_deref() {
            tracing::info!("ручная загрузка файла: {name}");
            schedule::run_job(&cfg, Some(name), cli.skip_agent).await?;
            return Ok(());
        }

        tracing::info!("разовый автопоиск пакета (4 файла) и загрузка");
        schedule::run_job(&cfg, None, cli.skip_agent).await?;
        return Ok(());
    }

    let cfg = Config::load()?;
    schedule::run_daemon(cfg, log_file, cli.skip_agent).await
}

/// Обнуляет лог-файл перед прогоном, чтобы остались только записи текущего запуска.
fn begin_run(log_file: &LogFile) -> Result<()> {
    log_file.reset()?;
    tracing::info!(
        "=== новый запуск okpo, лог: {} ===",
        log_file.path().display()
    );
    Ok(())
}
