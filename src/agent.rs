//! OpenSSH-сессия с reverse SOCKS (`-R port`) и запуск okpo-agent на Ubuntu.
//!
//! `ssh -R 3128` (только порт) поднимает на remote SOCKS5; трафик идёт через эту Windows-машину.
//! russh умеет обычный remote forward, но не remote-dynamic SOCKS как OpenSSH — поэтому здесь CLI `ssh`.

use std::process::Stdio;

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::config::{AgentConfig, SshConfig};

/// Экранирование для remote `sh -c` / аргумента ssh.
fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Дописывает `--skip-register-sync` к remote_command, если ещё нет.
fn remote_command_with_skip_sync(base: &str, skip_register_sync: bool) -> String {
    let base = base.trim();
    if !skip_register_sync {
        return base.to_string();
    }
    if base.split_whitespace().any(|t| t == "--skip-register-sync") {
        return base.to_string();
    }
    format!("{base} --skip-register-sync")
}

/// `ssh -R <port> … "cd <workdir> && <remote_command>"` — дождаться завершения.
///
/// Если `skip_register_sync`, к команде на Ubuntu дописывается `--skip-register-sync`
/// (файлы уже залиты по SFTP; agent не должен брать их с mount).
pub async fn run_register_with_reverse_socks(
    ssh: &SshConfig,
    agent: &AgentConfig,
    skip_register_sync: bool,
) -> Result<()> {
    if !agent.enabled {
        tracing::info!("agent.enabled=false — запуск okpo-agent на Ubuntu пропущен");
        return Ok(());
    }

    let remote_cmd = remote_command_with_skip_sync(agent.remote_command.trim(), skip_register_sync);
    let remote_script = format!(
        "cd {} && {}",
        sh_single_quote(agent.working_directory.trim()),
        remote_cmd
    );

    let target = format!("{}@{}", ssh.user, ssh.host);
    // Как вручную: `ssh -R 3128` — remote dynamic SOCKS на prod (не host:hostport).
    let forward_spec = agent.remote_socks_port.to_string();

    tracing::info!(
        target = %target,
        remote_socks_port = agent.remote_socks_port,
        skip_register_sync,
        command = %remote_script,
        "SSH: reverse SOCKS (-R) + запуск okpo-agent на Ubuntu"
    );

    let mut cmd = Command::new(&agent.ssh_binary);
    cmd.arg("-R")
        .arg(&forward_spec)
        .arg("-i")
        .arg(&ssh.private_key)
        .arg("-p")
        .arg(ssh.port.to_string())
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ServerAliveInterval=60")
        .arg("-o")
        .arg("ServerAliveCountMax=10")
        .arg(&target)
        .arg(&remote_script)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);

    let status = cmd
        .status()
        .await
        .with_context(|| {
            format!(
                "не удалось запустить `{}` (нужен OpenSSH-клиент в PATH)",
                agent.ssh_binary
            )
        })?;

    if !status.success() {
        bail!(
            "удалённый okpo-agent завершился с кодом {:?} (SSH target {})",
            status.code(),
            target
        );
    }

    tracing::info!("okpo-agent на Ubuntu завершился успешно; SSH-сессия с -R закрыта");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths_with_spaces() {
        assert_eq!(sh_single_quote("/home/a/okpo-agent"), "'/home/a/okpo-agent'");
        assert_eq!(sh_single_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn appends_skip_register_sync_once() {
        let base = "OKPO_SKIP_BUILD=1 ./run.sh prod register --dadata-parse";
        assert_eq!(remote_command_with_skip_sync(base, false), base);
        assert_eq!(
            remote_command_with_skip_sync(base, true),
            format!("{base} --skip-register-sync")
        );
        let already = format!("{base} --skip-register-sync");
        assert_eq!(remote_command_with_skip_sync(&already, true), already);
    }
}
