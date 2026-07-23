use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub source: SourceConfig,
    pub schedule: ScheduleConfig,
    pub ssh: SshConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceConfig {
    pub base_unc: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleConfig {
    pub timezone: String,
    pub hour: u32,
    pub minute: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub private_key: String,
    pub remote_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: SourceConfig {
                base_unc: String::from(
                    r"\\mskfs.rusagrotrans.ru\Groups\Департамент транспортно-экспедиционного обслуживания\ДТЭО new\Реестры",
                ),
            },
            schedule: ScheduleConfig {
                timezone: String::from("Europe/Moscow"),
                hour: 7,
                minute: 0,
            },
            ssh: SshConfig {
                host: String::from("10.101.139.4"),
                port: 22,
                user: String::from("atretyakov"),
                private_key: String::from(r"C:\Users\tretyakov_av\.ssh\id_rsa"),
                remote_dir: String::from("/home/atretyakov/okpo-agent/data/registers"),
            },
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let candidates = [
            PathBuf::from("config.toml"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.toml"),
        ];

        for path in &candidates {
            if path.is_file() {
                return Self::from_file(path);
            }
        }

        tracing::warn!("config.toml не найден, используются значения по умолчанию");
        Ok(Self::default())
    }

    fn from_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("не удалось прочитать {}", path.display()))?;
        let cfg: Config = toml::from_str(&text)
            .with_context(|| format!("не удалось разобрать {}", path.display()))?;
        cfg.validate()?;
        tracing::info!("конфигурация загружена из {}", path.display());
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.schedule.hour > 23 {
            bail!("schedule.hour должен быть 0..=23");
        }
        if self.schedule.minute > 59 {
            bail!("schedule.minute должен быть 0..=59");
        }
        if self.source.base_unc.trim().is_empty() {
            bail!("source.base_unc не должен быть пустым");
        }
        if self.ssh.remote_dir.trim().is_empty() {
            bail!("ssh.remote_dir не должен быть пустым");
        }
        Ok(())
    }

    pub fn tmp_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp")
    }
}
