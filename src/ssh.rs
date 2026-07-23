use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use russh::keys::{PrivateKeyWithHashAlg, load_secret_key};
use russh_sftp::client::SftpSession;

use crate::config::SshConfig;

pub struct ClientHandler;

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub async fn connect_sftp(cfg: &SshConfig) -> Result<(russh::client::Handle<ClientHandler>, SftpSession)> {
    tracing::info!("подключение к {}:{}...", cfg.host, cfg.port);
    let config = Arc::new(russh::client::Config::default());
    let mut session =
        russh::client::connect(config, (cfg.host.as_str(), cfg.port), ClientHandler)
            .await
            .with_context(|| format!("не удалось подключиться к {}:{}", cfg.host, cfg.port))?;

    let keypair = load_secret_key(Path::new(&cfg.private_key), None)
        .with_context(|| format!("не удалось загрузить ключ {}", cfg.private_key))?;

    let auth_res = session
        .authenticate_publickey(
            cfg.user.as_str(),
            PrivateKeyWithHashAlg::new(
                Arc::new(keypair),
                session.best_supported_rsa_hash().await?.flatten(),
            ),
        )
        .await
        .context("ошибка authenticate_publickey")?;

    if !auth_res.success() {
        bail!("не удалось авторизоваться по SSH-ключу для пользователя {}", cfg.user);
    }
    tracing::info!("SSH-авторизация успешна");

    let channel = session.channel_open_session().await.context("channel_open_session")?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .context("request_subsystem sftp")?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .context("SftpSession::new")?;

    Ok((session, sftp))
}
