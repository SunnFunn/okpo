use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use russh_sftp::protocol::OpenFlags;
use tokio::io::AsyncWriteExt;

use crate::config::{Config, SshConfig};
use crate::ssh;

/// Копирует файл в `tmp/` с исходным именем и заливает на Ubuntu по SFTP.
pub async fn copy_and_upload(cfg: &Config, source: &Path) -> Result<()> {
    let filename = source
        .file_name()
        .and_then(|n| n.to_str())
        .context("у исходного файла нет имени")?;

    let tmp_dir = Config::tmp_dir();
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("не удалось создать {}", tmp_dir.display()))?;

    let local_tmp = tmp_dir.join(filename);
    tracing::info!(
        "копирование {} -> {}",
        source.display(),
        local_tmp.display()
    );
    fs::copy(source, &local_tmp)
        .with_context(|| format!("не удалось скопировать {}", source.display()))?;

    upload_local_file(&cfg.ssh, &local_tmp, filename).await?;

    if let Err(err) = fs::remove_file(&local_tmp) {
        tracing::warn!(
            "не удалось удалить временный файл {}: {err}",
            local_tmp.display()
        );
    } else {
        tracing::info!("временный файл удалён: {}", local_tmp.display());
    }

    Ok(())
}

async fn upload_local_file(ssh_cfg: &SshConfig, local_path: &Path, filename: &str) -> Result<()> {
    let remote_path = format!(
        "{}/{}",
        ssh_cfg.remote_dir.trim_end_matches('/'),
        filename
    );

    let file_content = fs::read(local_path)
        .with_context(|| format!("не удалось прочитать {}", local_path.display()))?;

    let (_session, sftp) = ssh::connect_sftp(ssh_cfg).await?;

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

/// Утилита для тестов/диагностики путей.
#[allow(dead_code)]
pub fn local_tmp_path(filename: &str) -> PathBuf {
    Config::tmp_dir().join(filename)
}
