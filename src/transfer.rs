use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use regex::Regex;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::{Config, SshConfig, TestCheckConfig};
use crate::ssh;

/// Копирует один файл в `tmp/` и заливает на Ubuntu по SFTP.
pub async fn copy_and_upload(cfg: &Config, source: &Path) -> Result<()> {
    copy_and_upload_many(cfg, &[source.to_path_buf()]).await
}

/// Копирует пакет файлов в `tmp/` и заливает на Ubuntu одним SFTP-сеансом.
pub async fn copy_and_upload_many(cfg: &Config, sources: &[PathBuf]) -> Result<()> {
    if sources.is_empty() {
        anyhow::bail!("пустой пакет файлов для загрузки");
    }

    let tmp_dir = Config::tmp_dir();
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("не удалось создать {}", tmp_dir.display()))?;

    let mut staged: Vec<(PathBuf, String)> = Vec::with_capacity(sources.len());
    for source in sources {
        let filename = source
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| format!("у файла нет имени: {}", source.display()))?
            .to_string();

        let local_tmp = tmp_dir.join(&filename);
        tracing::info!(
            "копирование {} -> {}",
            source.display(),
            local_tmp.display()
        );
        fs::copy(source, &local_tmp)
            .with_context(|| format!("не удалось скопировать {}", source.display()))?;
        staged.push((local_tmp, filename));
    }

    let upload_result = upload_staged_files(&cfg.ssh, &staged).await;

    if upload_result.is_ok() {
        for (local_tmp, _) in &staged {
            if let Err(err) = fs::remove_file(local_tmp) {
                tracing::warn!(
                    "не удалось удалить временный файл {}: {err}",
                    local_tmp.display()
                );
            } else {
                tracing::info!("временный файл удалён: {}", local_tmp.display());
            }
        }
    } else {
        tracing::warn!(
            "загрузка пакета не удалась — файлы оставлены в {}",
            tmp_dir.display()
        );
    }

    upload_result
}

async fn upload_staged_files(ssh_cfg: &SshConfig, staged: &[(PathBuf, String)]) -> Result<()> {
    let (_session, sftp) = ssh::connect_sftp(ssh_cfg).await?;

    for (local_path, filename) in staged {
        upload_one(&sftp, ssh_cfg, local_path, filename).await?;
    }

    tracing::info!("пакет из {} файл(ов) успешно доставлен на Ubuntu", staged.len());
    Ok(())
}

async fn upload_one(
    sftp: &SftpSession,
    ssh_cfg: &SshConfig,
    local_path: &Path,
    filename: &str,
) -> Result<()> {
    let remote_path = format!(
        "{}/{}",
        ssh_cfg.remote_dir.trim_end_matches('/'),
        filename
    );

    let file_content = fs::read(local_path)
        .with_context(|| format!("не удалось прочитать {}", local_path.display()))?;

    match sftp.metadata(&remote_path).await {
        Ok(_) => {
            tracing::warn!(
                "файл уже есть на Ubuntu ({}), будет перезаписан",
                remote_path
            );
        }
        Err(_) => {
            tracing::info!("remote-файл отсутствует, создаём {}", remote_path);
        }
    }

    tracing::info!("загрузка на Ubuntu: {}", remote_path);
    let mut remote_file = sftp
        .open_with_flags(
            &remote_path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .with_context(|| format!("не удалось открыть remote-файл {remote_path}"))?;

    remote_file
        .write_all(&file_content)
        .await
        .context("ошибка записи SFTP")?;
    remote_file.flush().await.context("ошибка flush SFTP")?;
    remote_file
        .shutdown()
        .await
        .context("ошибка shutdown SFTP")?;

    tracing::info!("файл успешно доставлен: {}", remote_path);
    Ok(())
}

/// Парсит дату из имени `register_export_YYYY-MM-DD.json`.
pub fn parse_export_date(filename: &str, prefix: &str) -> Option<NaiveDate> {
    parse_export_date_inner(filename, prefix)
}

fn parse_export_date_inner(filename: &str, prefix: &str) -> Option<NaiveDate> {
    let escaped = regex::escape(prefix);
    let re = Regex::new(&format!(r"^{escaped}_(\d{{4}})-(\d{{2}})-(\d{{2}})\.json$")).ok()?;
    let caps = re.captures(filename)?;
    let year: i32 = caps.get(1)?.as_str().parse().ok()?;
    let month: u32 = caps.get(2)?.as_str().parse().ok()?;
    let day: u32 = caps.get(3)?.as_str().parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

/// Выбирает файл с максимальной датой в имени; при равенстве — по имени.
pub fn pick_latest_export(names: &[String], prefix: &str) -> Option<(NaiveDate, String)> {
    let mut best: Option<(NaiveDate, String)> = None;
    for name in names {
        let Some(date) = parse_export_date_inner(name, prefix) else {
            continue;
        };
        match &best {
            Some((best_date, best_name)) if date < *best_date || (date == *best_date && name <= best_name) => {}
            _ => best = Some((date, name.clone())),
        }
    }
    best
}

/// Скачивает самый свежий `register_export_*.json` с прод в `tmp/`.
pub async fn download_latest_export(
    ssh: &SshConfig,
    tc: &TestCheckConfig,
) -> Result<(PathBuf, NaiveDate)> {
    let remote_dir = tc.remote_exports_dir.trim_end_matches('/').to_string();
    tracing::info!("SFTP: список экспортов в {remote_dir}");

    let (_session, sftp) = ssh::connect_sftp(ssh).await?;
    let entries = sftp
        .read_dir(&remote_dir)
        .await
        .with_context(|| format!("не удалось прочитать каталог {remote_dir}"))?;

    let names: Vec<String> = entries
        .map(|entry| entry.file_name())
        .filter(|name| name != "." && name != "..")
        .collect();

    let (date, filename) = pick_latest_export(&names, &tc.export_prefix).with_context(|| {
        format!(
            "в {remote_dir} нет файлов вида {}_YYYY-MM-DD.json",
            tc.export_prefix
        )
    })?;

    let remote_path = format!("{remote_dir}/{filename}");
    tracing::info!(
        "выбран экспорт {} (дата {}), скачивание...",
        remote_path,
        date
    );

    let mut remote_file = sftp
        .open(&remote_path)
        .await
        .with_context(|| format!("не удалось открыть {remote_path}"))?;

    let mut buf = Vec::new();
    remote_file
        .read_to_end(&mut buf)
        .await
        .with_context(|| format!("ошибка чтения {remote_path}"))?;

    let tmp_dir = Config::tmp_dir();
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("не удалось создать {}", tmp_dir.display()))?;
    let local_path = tmp_dir.join(&filename);
    fs::write(&local_path, &buf)
        .with_context(|| format!("не удалось записать {}", local_path.display()))?;

    tracing::info!(
        "экспорт сохранён локально: {} ({} байт)",
        local_path.display(),
        buf.len()
    );
    Ok((local_path, date))
}

/// Утилита для тестов/диагностики путей.
#[allow(dead_code)]
pub fn local_tmp_path(filename: &str) -> PathBuf {
    Config::tmp_dir().join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_export_name() {
        let d = parse_export_date("register_export_2026-07-27.json", "register_export");
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 7, 27));
    }

    #[test]
    fn parse_rejects_wrong_prefix() {
        assert!(parse_export_date("other_export_2026-07-27.json", "register_export").is_none());
        assert!(parse_export_date("register_export_2026-07-27.xls", "register_export").is_none());
        assert!(parse_export_date("garbage.json", "register_export").is_none());
    }

    #[test]
    fn pick_latest_by_date() {
        let names = vec![
            "register_export_2026-07-20.json".into(),
            "register_export_2026-07-27.json".into(),
            "notes.txt".into(),
            "register_export_2026-07-25.json".into(),
        ];
        let (date, name) = pick_latest_export(&names, "register_export").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 7, 27).unwrap());
        assert_eq!(name, "register_export_2026-07-27.json");
    }

    #[test]
    fn pick_latest_empty() {
        assert!(pick_latest_export(&[], "register_export").is_none());
    }
}
