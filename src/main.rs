use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use russh::keys::{PrivateKeyWithHashAlg, load_secret_key};
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- НАСТРОЙКИ ---
    let source_file = r"\\mskfs.rusagrotrans.ru\Groups\Департамент транспортно-экспедиционного обслуживания\ДТЭО new\Реестры\2026\Июль 26\Реестр 22.07..xls";

    let ubuntu_host = "10.101.139.4";
    let ubuntu_user = "atretyakov";
    let ubuntu_path = "/home/atretyakov/okpo-agent/data/registers/tmp.xls";

    let private_key_path = r"C:\Users\tretyakov_av\.ssh\id_rsa";
    // ------------------

    let mut local_temp_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    local_temp_path.push("tmp");
    if !local_temp_path.exists() {
        fs::create_dir_all(&local_temp_path)?;
    }
    local_temp_path.push("tmp.xls");

    // 1. Копирование файла локально из сети Windows
    println!("Шаг 1: Копирование файла из сети во временную папку...");
    fs::copy(source_file, &local_temp_path)?;
    println!("Файл успешно скопирован локально.");

    let file_content = fs::read(&local_temp_path)?;

    // 2. Подключение к SSH
    println!("Шаг 2: Подключение к машине Ubuntu и Handshake...");
    let config = Arc::new(russh::client::Config::default());
    let mut session = russh::client::connect(config, (ubuntu_host, 22), ClientHandler).await?;

    println!("Чтение и парсинг SSH-ключа...");
    let keypair = load_secret_key(private_key_path, None)?;

    println!("Авторизация по SSH-ключу...");
    let auth_res = session
        .authenticate_publickey(
            ubuntu_user,
            PrivateKeyWithHashAlg::new(
                Arc::new(keypair),
                session.best_supported_rsa_hash().await?.flatten(),
            ),
        )
        .await?;

    if !auth_res.success() {
        return Err("Ошибка: Не удалось авторизоваться по SSH-ключу!".into());
    }
    println!("Авторизация успешна.");

    // 3. Отправка файла через SFTP
    println!("Шаг 3: Отправка файла на Ubuntu через SFTP...");
    let channel = session.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream()).await?;

    {
        let mut remote_file = sftp.create(ubuntu_path).await?;
        remote_file.write_all(&file_content).await?;
        remote_file.flush().await?;
        remote_file.shutdown().await?;
    }

    println!("Файл успешно доставлен на Ubuntu!");

    // 4. Очистка временного файла
    fs::remove_file(&local_temp_path)?;
    println!("Временный локальный файл удален. Скрипт завершен.");

    Ok(())
}

struct ClientHandler;

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}
