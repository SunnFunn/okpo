use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use futures_util::AsyncWriteExt; // Для асинхронной записи в sftp-файл

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- НАСТРОЙКИ ---
    let source_file = r"\\mskfs.rusagrotrans.ru\Groups\Департамент транспортно-экспедиционного обслуживания\ДТЭО new\Реестры\2026\Июль 26\Реестр 22.07..xls";
    
    let ubuntu_host = "10.101.139.4"; 
    let ubuntu_user = "atretyakov";
    let ubuntu_path = "/home/atretyakov/okpo-agent/data/registers/tmp.xls"; 

    let private_key_path = r"C:\Users\tretyakov_av\.ssh\id_rsa"; 
    // ------------------

    // Динамически собираем локальный путь в tmp
    let mut local_temp_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    local_temp_path.push("tmp");

    if !local_temp_path.exists() {
        fs::create_dir_all(&local_temp_path)?;
    }
    local_temp_path.push("tmp.xls");

    // 1. Копирование файла локально из сети Windows
    println!("Шаг 1: Копирование файла из сети во временную папку проекта...");
    fs::copy(source_file, &local_temp_path)?;
    println!("Файл успешно скопирован локально.");

    // Читаем локальный файл в память
    let file_content = fs::read(&local_temp_path)?;

    // 2. Инициализация и подключение к SSH (Чистый Rust)
    println!("Шаг 2: Подключение к машине Ubuntu и Handshake...");
    let config = russh::client::Config::default();
    let config = Arc::new(config);
    let sh = ClientHandler;
    
    // В версии 0.62 коннект возвращает готовую сессию
    let mut session = russh::client::connect(config, (ubuntu_host, 22), sh).await?;
    
    // Загрузка приватного ключа через встроенный парсер russh
    println!("Чтение и парсинг SSH-ключа...");
    let key_str = fs::read_to_string(private_key_path)?;
    let keypair = russh::keys::decode_secret_key(&key_str, None)?;
    
    println!("Авторизация по SSH-ключу...");
    let auth_res = session.authenticate_publickey(ubuntu_user, Arc::new(keypair)).await?;
    
    if !auth_res {
        return Err("Ошибка: Не удалось авторизоваться по SSH-ключу!".into());
    }
    println!("Авторизация успешна.");

    // 3. Отправка файла через SFTP подсистему (russh-sftp 2.3)
    println!("Шаг 3: Отправка файла на Ubuntu через SFTP...");
    
    // Открываем канал для подсистемы sftp
    let channel = session.channel_open_session().await?;
    let sftp = russh_sftp::client::SftpSession::new(channel).await?;
    
    // Ограничиваем область видимости файла фигурной скобкой,
    // чтобы при выходе из неё файл закрылся сам (Drop)
    {
        let mut remote_file = sftp.create(ubuntu_path).await?;
        // Записываем буфер в удаленный файл асинхронно
        remote_file.write_all(&file_content).await?;
        println!("Данные записаны в буфер передачи...");
    } // <-- Здесь remote_file уничтожается, и russh-sftp автоматически закрывает файл на сервере

    println!("Файл успешно доставлен на Ubuntu!");

    // 4. Очистка временного файла
    fs::remove_file(&local_temp_path)?;
    println!("Временный локальный файл удален. Скрипт завершен.");

    Ok(())
}

// Хэндлер для сессии клиента russh v0.62
struct ClientHandler;

// В версии 0.62 макрос #[async_trait] больше НЕ НУЖЕН
impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    // Обновленная сигнатура метода с правильными типами из russh 0.62
    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Автоматически доверяем ключу удаленного сервера Ubuntu
        Ok(true)
    }
}

