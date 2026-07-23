use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::AsyncWriteExt;

use crate::config::{Config, SshConfig};
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

/// Утилита для тестов/диагностики путей.
#[allow(dead_code)]
pub fn local_tmp_path(filename: &str) -> PathBuf {
    Config::tmp_dir().join(filename)
}
