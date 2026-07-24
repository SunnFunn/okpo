use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

/// Имя лог-файла в корне проекта (только последний запуск).
pub const LOG_FILE_NAME: &str = "okpo-task.log";

/// Перезаписываемый лог-файл: при каждом [`LogFile::reset`] содержимое обнуляется.
#[derive(Clone)]
pub struct LogFile {
    path: PathBuf,
    inner: Arc<Mutex<File>>,
}

impl LogFile {
    pub fn create(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("не удалось создать каталог {}", parent.display()))?;
            }
        }
        let file = open_truncate(&path)?;
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(file)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Заменяет файл новой пустой записью (логи предыдущего запуска удаляются).
    pub fn reset(&self) -> Result<()> {
        let file = open_truncate(&self.path)?;
        let mut guard = self
            .inner
            .lock()
            .expect("лог-файл: мьютекс отравлен");
        *guard = file;
        Ok(())
    }
}

fn open_truncate(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .with_context(|| format!("не удалось открыть лог {}", path.display()))
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| io::Error::other(format!("лог-файл: {e}")))?;
        guard.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| io::Error::other(format!("лог-файл: {e}")))?;
        guard.flush()
    }
}

impl<'a> MakeWriter<'a> for LogFile {
    type Writer = LogFile;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Путь к логу: рядом с `config.toml` (cwd или корень проекта).
pub fn resolve_log_path() -> PathBuf {
    let cwd = PathBuf::from(".").join(LOG_FILE_NAME);
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(LOG_FILE_NAME);

    if Path::new("config.toml").is_file() {
        return cwd;
    }
    if PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("config.toml")
        .is_file()
    {
        return manifest;
    }
    cwd
}

/// Инициализация tracing: stdout + файл (файл создаётся с обнулением).
pub fn init() -> Result<LogFile> {
    let path = resolve_log_path();
    let log_file = LogFile::create(&path)?;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let writer = log_file.clone().and(std::io::stdout);

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();

    Ok(log_file)
}
