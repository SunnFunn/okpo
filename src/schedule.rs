use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::agent;
use crate::config::{Config, ScheduleConfig};
use crate::discover;
use crate::logging::LogFile;
use crate::transfer;

/// Сегодняшняя дата в таймзоне из конфига.
pub fn today_in_tz(tz_name: &str) -> Result<NaiveDate> {
    let tz = Tz::from_str(tz_name).with_context(|| format!("неизвестная таймзона {tz_name}"))?;
    Ok(Utc::now().with_timezone(&tz).date_naive())
}

/// Один прогон: пакет из 4 реестров или один файл по `--file`, затем (опционально) okpo-agent.
pub async fn run_job(cfg: &Config, file: Option<&str>, skip_agent: bool) -> Result<()> {
    let today = today_in_tz(&cfg.schedule.timezone)?;
    match file {
        Some(name) => {
            let source = discover::resolve_by_filename(&cfg.source.base_unc, name, today)?;
            transfer::copy_and_upload(cfg, &source).await?;
        }
        None => {
            let package = discover::discover_latest_package(&cfg.source.base_unc, today)?;
            tracing::info!(
                "к загрузке пакет из {} файл(ов)",
                package.len()
            );
            transfer::copy_and_upload_many(cfg, &package).await?;
        }
    }

    if skip_agent {
        tracing::info!("--skip-agent: запуск okpo-agent на Ubuntu пропущен");
        return Ok(());
    }

    agent::run_register_with_reverse_socks(&cfg.ssh, &cfg.agent).await
}

/// Долгоживущий демон: каждый день в hour:minute по таймзоне из конфига.
pub async fn run_daemon(cfg: Config, log_file: LogFile, skip_agent: bool) -> Result<()> {
    let tz = Tz::from_str(&cfg.schedule.timezone)
        .with_context(|| format!("неизвестная таймзона {}", cfg.schedule.timezone))?;

    tracing::info!(
        "демон запущен: ежедневно в {:02}:{:02} ({})",
        cfg.schedule.hour,
        cfg.schedule.minute,
        cfg.schedule.timezone
    );

    loop {
        let sleep_for = duration_until_next_run(&cfg.schedule, tz)?;
        tracing::info!(
            "следующий запуск через {} сек (~{:.1} ч)",
            sleep_for.as_secs(),
            sleep_for.as_secs_f64() / 3600.0
        );
        tokio::time::sleep(sleep_for).await;

        // Каждый суточный прогон перезаписывает лог-файл.
        if let Err(err) = log_file.reset() {
            tracing::error!("не удалось обнулить лог-файл: {err:#}");
        } else {
            tracing::info!(
                "=== новый запуск okpo, лог: {} ===",
                log_file.path().display()
            );
        }

        tracing::info!("старт ежедневной выгрузки");
        if let Err(err) = run_job(&cfg, None, skip_agent).await {
            tracing::error!("ошибка ежедневной выгрузки: {err:#}");
        } else {
            tracing::info!("ежедневная выгрузка завершена успешно");
        }

        // Небольшая пауза, чтобы не зациклиться в ту же минуту.
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

fn duration_until_next_run(schedule: &ScheduleConfig, tz: Tz) -> Result<Duration> {
    let now = Utc::now().with_timezone(&tz);
    let time = NaiveTime::from_hms_opt(schedule.hour, schedule.minute, 0)
        .context("некорректное время расписания")?;

    let mut candidate_date = now.date_naive();
    let mut candidate = match tz.from_local_datetime(&candidate_date.and_time(time)) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => bail!("некорректное локальное время расписания"),
    };

    if candidate <= now {
        candidate_date = candidate_date
            .succ_opt()
            .context("не удалось вычислить следующий день")?;
        candidate = match tz.from_local_datetime(&candidate_date.and_time(time)) {
            LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt,
            LocalResult::None => bail!("некорректное локальное время на следующий день"),
        };
    }

    let delta = candidate
        .signed_duration_since(now)
        .to_std()
        .unwrap_or(Duration::from_secs(1));
    Ok(delta.max(Duration::from_secs(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::Moscow;

    #[test]
    fn next_run_is_in_future() {
        let schedule = ScheduleConfig {
            timezone: "Europe/Moscow".into(),
            hour: 7,
            minute: 0,
        };
        let d = duration_until_next_run(&schedule, Moscow).unwrap();
        assert!(d.as_secs() >= 1);
        assert!(d.as_secs() <= 24 * 3600 + 60);
    }
}
