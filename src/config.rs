use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub source: SourceConfig,
    pub schedule: ScheduleConfig,
    pub ssh: SshConfig,
    /// После SFTP: reverse SOCKS + запуск okpo-agent на Ubuntu (опционально).
    #[serde(default)]
    pub agent: AgentConfig,
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

/// Запуск `okpo-agent` на prod в той же логической цепочке, что и выгрузка реестров.
///
/// Реализуется через OpenSSH CLI: `ssh -R <port> … remote_command`
/// (remote dynamic SOCKS; трафик DaData/Yandex идёт через эту Windows-машину).
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    /// Если false — только SFTP, без запуска бинаря на Ubuntu.
    #[serde(default = "default_agent_enabled")]
    pub enabled: bool,
    /// Порт reverse SOCKS на Ubuntu (`ssh -R <port>`).
    #[serde(default = "default_socks_port")]
    pub remote_socks_port: u16,
    /// Каталог okpo-agent на Ubuntu (туда `cd` перед командой).
    #[serde(default = "default_workdir")]
    pub working_directory: String,
    /// Команда на Ubuntu (после `cd working_directory`).
    #[serde(default = "default_remote_command")]
    pub remote_command: String,
    /// Имя OpenSSH-клиента в PATH (`ssh` / `ssh.exe`).
    #[serde(default = "default_ssh_binary")]
    pub ssh_binary: String,
}

fn default_agent_enabled() -> bool {
    true
}

fn default_socks_port() -> u16 {
    3128
}

fn default_workdir() -> String {
    String::from("/home/atretyakov/okpo-agent")
}

fn default_remote_command() -> String {
    String::from("OKPO_SKIP_BUILD=1 ./run.sh prod register --dadata-parse")
}

fn default_ssh_binary() -> String {
    String::from("ssh")
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: default_agent_enabled(),
            remote_socks_port: default_socks_port(),
            working_directory: default_workdir(),
            remote_command: default_remote_command(),
            ssh_binary: default_ssh_binary(),
        }
    }
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
            agent: AgentConfig::default(),
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
        if self.agent.enabled {
            if self.agent.remote_socks_port == 0 {
                bail!("agent.remote_socks_port должен быть > 0");
            }
            if self.agent.working_directory.trim().is_empty() {
                bail!("agent.working_directory не должен быть пустым");
            }
            if self.agent.remote_command.trim().is_empty() {
                bail!("agent.remote_command не должен быть пустым");
            }
        }
        Ok(())
    }

    pub fn tmp_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp")
    }
}
