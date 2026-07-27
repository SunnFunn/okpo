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
    about = "Запуск okpo-agent на Ubuntu (ssh -R SOCKS); опционально старая выгрузка реестров по SFTP"
)]
struct Cli {
    /// Один прогон без ожидания расписания
    #[arg(long, conflicts_with = "file")]
    once: bool,

    /// Ручная загрузка одного файла по имени (включает SFTP; как --skip-register-sync)
    #[arg(long, value_name = "NAME")]
    file: Option<String>,

    /// Только SFTP, без `ssh -R` и без запуска okpo-agent на Ubuntu
    #[arg(long)]
    skip_agent: bool,

    /// Запасной режим: отбор + SFTP с UNC, а okpo-agent на Ubuntu — с `--skip-register-sync`
    /// (без mount). По умолчанию SFTP не делается: agent сам берёт файлы с `/data/registers`.
    #[arg(long)]
    skip_register_sync: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_file = logging::init()?;
    let cli = Cli::parse();

    // --file всегда подразумевает выгрузку по SFTP (старый путь).
    let skip_register_sync = cli.skip_register_sync || cli.file.is_some();

    if cli.file.is_some() || cli.once {
        begin_run(&log_file)?;
        let cfg = Config::load()?;

        if let Some(name) = cli.file.as_deref() {
            tracing::info!("ручная загрузка файла: {name}");
            schedule::run_job(&cfg, Some(name), cli.skip_agent, skip_register_sync).await?;
            return Ok(());
        }

        if skip_register_sync {
            tracing::info!("разовый прогон: SFTP-пакет + okpo-agent (--skip-register-sync)");
        } else {
            tracing::info!("разовый прогон: только okpo-agent (файлы с mount на Ubuntu)");
        }
        schedule::run_job(&cfg, None, cli.skip_agent, skip_register_sync).await?;
        return Ok(());
    }

    let cfg = Config::load()?;
    schedule::run_daemon(cfg, log_file, cli.skip_agent, skip_register_sync).await
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
