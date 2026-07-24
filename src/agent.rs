//! После SFTP: OpenSSH-сессия с reverse SOCKS (`-R port`) и запуск okpo-agent на Ubuntu.
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

/// `ssh -R <port> … "cd <workdir> && <remote_command>"` — дождаться завершения.
pub async fn run_register_with_reverse_socks(ssh: &SshConfig, agent: &AgentConfig) -> Result<()> {
    if !agent.enabled {
        tracing::info!("agent.enabled=false — запуск okpo-agent на Ubuntu пропущен");
        return Ok(());
    }

    let remote_script = format!(
        "cd {} && {}",
        sh_single_quote(agent.working_directory.trim()),
        agent.remote_command.trim()
    );

    let target = format!("{}@{}", ssh.user, ssh.host);
    // Как вручную: `ssh -R 3128` — remote dynamic SOCKS на prod (не host:hostport).
    let forward_spec = agent.remote_socks_port.to_string();

    tracing::info!(
        target = %target,
        remote_socks_port = agent.remote_socks_port,
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
        assert_eq!(
            sh_single_quote("it's"),
            "'it'\\''s'"
        );
    }
}
